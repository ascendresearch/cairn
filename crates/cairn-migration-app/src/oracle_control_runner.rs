use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
    time::Duration,
};

use cairn_execution::{
    CapturePolicy, CommandContract, DOCKER_BACKEND, DiagnosticByteLimit,
    DockerExecutionEnvironmentV1, DockerImageId, EvidenceByteLimit, ExecutionBackend,
    ExecutionEnvironmentArtifact, ExecutionJob, ExecutionJobState, ExecutionOutcome,
    ExecutionTimeoutMillis, InputBundleArtifact, InputBundleEntry, InputBundleV1, InputFileMode,
    JobContract, NetworkPolicy, OutputByteLimit, PlacementRequest, ReservationReleaseReason,
    ResourceRequest, SandboxPath, WorkerPoolName, recover_execution_job,
};
use cairn_migration::{
    OracleAdmissionAttemptV1, OracleAdmissionEvidenceV1, OracleAdmissionMechanismCatalogV1,
    OracleAdmissionPolicyV1, OracleCheckPlanV1, OracleControlDispatchV1,
    OracleControlFailureClassV1, OracleControlFamilyV1, OracleControlReceiptV1,
    OracleControlResultV1, OracleControlRunV1, OracleControlRunnerArtifact,
    OracleControlWorkerBindingV1, OracleItemArtifact, OracleItemStatement, OracleItemV1,
    OracleMechanismQualificationReceiptV1, OracleObligationResolutionV1, OraclePortfolioProposalV1,
    OracleQualifiedMechanismArtifact, OracleQualifiedMechanismRegistrationV1,
    TrustedOracleControlObservationV1,
};
use cairn_protocol::{
    AssignmentId, AttemptId, CommandId, ContentId, ContentType, ControlMessageId, JobId, LeaseId,
    PlacementId, ReservationId,
};
use cairn_record::ContentStore;
use cairn_server::{
    ControllerScheduleCommandIds, ControllerScheduleIds, ControllerSchedulingOutcome, ServerConfig,
    release_execution_reservation, schedule_execution_contract,
};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

const RUNNER: &[u8] = br#"#!/bin/sh
set -eu
mode=$(sed -n '1p' meta/control-mode)
validate_plan() {
  file=$1
  item=$2
  digest=$3
  actual=$(sha256sum "$file" | cut -d ' ' -f 1)
  [ "$actual" = "$digest" ] || return 1
  grep -Fq '"schema_version":1' "$file" || return 1
  grep -Fq "\"item\":\"$item\"" "$file" || return 1
  grep -Eq '"method":"(static-analysis|reference-execution|metamorphic|boundary-probe|runtime-observation)"' "$file" || return 1
  grep -Eq '"objective":"[^"]+"' "$file" || return 1
  grep -Eq '"setup":"[^"]+"' "$file" || return 1
  grep -Eq '"observation":"[^"]+"' "$file" || return 1
  grep -Eq '"pass_condition":"[^"]+"' "$file" || return 1
  grep -Fq '"evidence":[' "$file" || return 1
}
while IFS='|' read -r file item digest; do
  [ -n "$file" ] || continue
  case "$mode" in
    mechanism-qualification|honest)
      validate_plan "$file" "$item" "$digest"
      ;;
    mutant|hidden|bypass)
      if validate_plan "$file" "$item" "$digest"; then
        exit 31
      fi
      ;;
    *) exit 32 ;;
  esac
done < meta/control-index
"#;
const MIN_SCHEDULING_RETRY_INTERVAL: Duration = Duration::from_secs(1);

/// Product configuration for one qualified Oracle control mechanism running on a Worker.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleControlWorkerConfigV1 {
    schema_version: u16,
    image: DockerImageId,
    worker_pool: WorkerPoolName,
    execution_timeout: ExecutionTimeoutMillis,
    poll_interval_ms: u64,
    completion_timeout_ms: u64,
}

impl OracleControlWorkerConfigV1 {
    fn validate(&self) -> Result<(), OracleControlRunnerError> {
        if self.schema_version != 1 || self.poll_interval_ms == 0 || self.completion_timeout_ms == 0
        {
            return Err(OracleControlRunnerError::InvalidConfiguration);
        }
        Ok(())
    }
}

struct QualifiedTaskV1 {
    proposal: ContentId<cairn_migration::OraclePortfolioProposalArtifact>,
    catalog: OracleAdmissionMechanismCatalogV1,
}

struct WorkerReceiptV1 {
    job_id: JobId,
    attempt_id: AttemptId,
    contract_id: ContentId<cairn_execution::JobContractArtifact>,
    receipt_id: ContentId<cairn_execution::ExecutionReceiptArtifact>,
    receipt: cairn_execution::ExecutionReceipt,
    result: OracleControlResultV1,
}

/// Product-owned adapter that qualifies and executes Oracle controls on the generic Worker path.
pub struct OracleControlRunnerV1 {
    server: ServerConfig,
    config: OracleControlWorkerConfigV1,
    qualified: BTreeMap<cairn_protocol::TaskId, QualifiedTaskV1>,
}

impl OracleControlRunnerV1 {
    /// Builds a Worker-backed runner after validating scheduler and worker configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the worker configuration is invalid or scheduling is disabled.
    pub fn new(
        server: ServerConfig,
        config: OracleControlWorkerConfigV1,
    ) -> Result<Self, OracleControlRunnerError> {
        config.validate()?;
        if server.scheduler.is_none() {
            return Err(OracleControlRunnerError::InvalidConfiguration);
        }
        Ok(Self {
            server,
            config,
            qualified: BTreeMap::new(),
        })
    }

    /// Qualifies every policy control family through one exact Worker observation.
    ///
    /// # Errors
    ///
    /// Returns an error for binding drift, scheduling failure, or an unsuccessful Worker receipt.
    pub async fn qualify(
        &mut self,
        task_id: cairn_protocol::TaskId,
        proposal: &OraclePortfolioProposalV1,
        policy: &OracleAdmissionPolicyV1,
        plans: &[OracleCheckPlanV1],
    ) -> Result<OracleAdmissionMechanismCatalogV1, OracleControlRunnerError> {
        validate_plan_coverage(proposal, plans)?;
        // The bundled runner currently proves only canonical plan encoding and authority binding.
        // Those checks are useful protocol controls, but they are not evidence that any plan can
        // execute against an Ascend-C candidate or target receipt. Fail closed until the ordinary
        // Worker path executes a candidate-facing Oracle mechanism; otherwise mechanical
        // Admission would turn a structural self-check into false semantic authority.
        if !candidate_facing_runner_available() {
            return Err(OracleControlRunnerError::SemanticExecutionUnavailable);
        }
        let proposal_id = proposal.identity().map_err(domain)?;
        if let Some(existing) = self.qualified.get(&task_id) {
            if existing.proposal == proposal_id {
                return Ok(existing.catalog.clone());
            }
        }
        let runner = self.runner_id()?;
        let qualification = self
            .execute_batch(OracleControlFamilyV1::MechanismQualification, plans, None)
            .await?;
        if qualification.result != OracleControlResultV1::Passed {
            return Err(OracleControlRunnerError::WorkerRejected);
        }
        let mut registrations = Vec::new();
        let mut content = self.content()?;
        for control in policy.required_controls() {
            let mechanism = Self::mechanism_id(*control, runner)?;
            let receipt = OracleMechanismQualificationReceiptV1::new(
                *control,
                mechanism,
                runner,
                qualification.receipt_id,
            );
            let receipt_id = receipt.identity().map_err(domain)?;
            archive_exact(&mut content, receipt_id, &receipt)?;
            registrations.push(OracleQualifiedMechanismRegistrationV1::new(
                *control, mechanism, runner, receipt_id,
            ));
        }
        let catalog = OracleAdmissionMechanismCatalogV1::new(registrations).map_err(domain)?;
        let catalog_id = catalog.identity().map_err(domain)?;
        tracing::info!(
            target: "cairn.migration.oracle-control",
            event = "oracle_control_mechanisms_qualified",
            task_id = %task_id,
            proposal_id = %proposal_id,
            catalog_id = %catalog_id,
            worker_job_id = %qualification.job_id,
            worker_attempt_id = %qualification.attempt_id,
            "Oracle control mechanisms qualified by Worker evidence"
        );
        self.qualified.insert(
            task_id,
            QualifiedTaskV1 {
                proposal: proposal_id,
                catalog: catalog.clone(),
            },
        );
        Ok(catalog)
    }

    /// Executes every exact admission obligation through the qualified Worker mechanisms.
    ///
    /// # Errors
    ///
    /// Returns an error for qualification drift, scheduling failure, or invalid observations.
    #[allow(
        clippy::too_many_lines,
        reason = "one linear control transaction keeps obligation, dispatch, observation, and receipt lineage visible"
    )]
    pub async fn execute_controls(
        &mut self,
        task_id: cairn_protocol::TaskId,
        proposal: &OraclePortfolioProposalV1,
        attempt: &OracleAdmissionAttemptV1,
        plans: &[OracleCheckPlanV1],
    ) -> Result<OracleAdmissionEvidenceV1, OracleControlRunnerError> {
        validate_plan_coverage(proposal, plans)?;
        let proposal_id = proposal.identity().map_err(domain)?;
        let catalog = self
            .qualified
            .get(&task_id)
            .ok_or(OracleControlRunnerError::NotQualified)?
            .catalog
            .clone();
        if self
            .qualified
            .get(&task_id)
            .is_none_or(|qualified| qualified.proposal != proposal_id)
            || attempt.proposal() != proposal_id
            || attempt.mechanisms() != catalog.identity().map_err(domain)?
        {
            return Err(OracleControlRunnerError::Binding);
        }
        let mut receipts = Vec::new();
        let controls = attempt
            .required_controls()
            .iter()
            .map(cairn_migration::OracleControlObligationV1::control)
            .collect::<BTreeSet<_>>();
        for control in controls {
            let obligations = attempt
                .required_controls()
                .iter()
                .filter(|obligation| obligation.control() == control)
                .collect::<Vec<_>>();
            for obligation in &obligations {
                let item_plans = plans
                    .iter()
                    .filter(|plan| plan.item() == obligation.item())
                    .cloned()
                    .collect::<Vec<_>>();
                if item_plans.is_empty() {
                    return Err(OracleControlRunnerError::Binding);
                }
                let item = proposal
                    .accepted_items()
                    .iter()
                    .find(|accepted| {
                        accepted
                            .item()
                            .identity()
                            .is_ok_and(|identity| identity == obligation.item())
                    })
                    .map(cairn_migration::OracleAcceptedItemV1::item)
                    .ok_or(OracleControlRunnerError::Binding)?;
                let worker = match self.execute_batch(control, &item_plans, Some(item)).await {
                    Ok(worker) => worker,
                    Err(
                        OracleControlRunnerError::NoWorker
                        | OracleControlRunnerError::WorkerRejected
                        | OracleControlRunnerError::WorkerTimeout,
                    ) => {
                        tracing::warn!(
                            target: "cairn.migration.oracle-control",
                            event = "oracle_control_observation_unavailable",
                            task_id = %task_id,
                            proposal_id = %proposal_id,
                            item_id = %obligation.item(),
                            control = ?control,
                            plan_count = item_plans.len(),
                            "qualified Oracle control produced no trusted observation; Admission will request infrastructure reconciliation"
                        );
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                let run = OracleControlRunV1::new(attempt, &catalog, (*obligation).clone())
                    .map_err(domain)?;
                let failure_class = classify_control_failure(
                    control,
                    worker.result,
                    worker.receipt.outcome(),
                    worker.receipt.exit_code(),
                );
                let dispatch = OracleControlDispatchV1::new(
                    &run,
                    OracleControlWorkerBindingV1::new(
                        worker.job_id,
                        worker.attempt_id,
                        worker.contract_id,
                    ),
                )
                .map_err(domain)?;
                let observation = TrustedOracleControlObservationV1::new(
                    &dispatch,
                    worker.receipt_id,
                    worker.receipt.clone(),
                    worker.result,
                )
                .map_err(domain)?;
                let receipt = OracleControlReceiptV1::from_trusted_observation(
                    proposal_id,
                    &run,
                    &observation,
                    failure_class,
                )
                .map_err(domain)?;
                if let Some(failure_class) = failure_class {
                    tracing::warn!(
                        target: "cairn.migration.oracle-control",
                        event = "oracle_control_failed",
                        task_id = %task_id,
                        proposal_id = %proposal_id,
                        item_id = %obligation.item(),
                        control = ?control,
                        failure_class = ?failure_class,
                        worker_receipt_id = %worker.receipt_id,
                        "qualified Oracle control failed with typed ownership"
                    );
                }
                let mut content = self.content()?;
                archive_exact(&mut content, run.identity().map_err(domain)?, &run)?;
                archive_exact(
                    &mut content,
                    dispatch.identity().map_err(domain)?,
                    &dispatch,
                )?;
                archive_exact(
                    &mut content,
                    observation.identity().map_err(domain)?,
                    &observation,
                )?;
                receipts.push(receipt);
            }
            tracing::info!(
                target: "cairn.migration.oracle-control",
                event = "oracle_control_family_completed",
                task_id = %task_id,
                control = ?control,
                obligation_count = obligations.len(),
                "qualified Oracle control family completed on Worker"
            );
        }
        OracleAdmissionEvidenceV1::new(attempt, receipts).map_err(domain)
    }

    fn runner_id(
        &self,
    ) -> Result<ContentId<OracleControlRunnerArtifact>, OracleControlRunnerError> {
        derive(&json!({
            "schema_version": 1,
            "implementation": "oracle-check-plan-structural-runner",
            "runner_sha256": sha256_hex(RUNNER),
            "image": self.config.image,
        }))
    }

    fn mechanism_id(
        control: OracleControlFamilyV1,
        runner: ContentId<OracleControlRunnerArtifact>,
    ) -> Result<ContentId<OracleQualifiedMechanismArtifact>, OracleControlRunnerError> {
        derive(&json!({"schema_version":1,"control":control,"runner":runner}))
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one linear scheduling transaction keeps all Worker receipt bindings visible"
    )]
    async fn execute_batch(
        &self,
        control: OracleControlFamilyV1,
        plans: &[OracleCheckPlanV1],
        bound_item: Option<&OracleItemV1>,
    ) -> Result<WorkerReceiptV1, OracleControlRunnerError> {
        if plans.is_empty() {
            return Err(OracleControlRunnerError::Binding);
        }
        let (bundle, bundle_bytes) = build_bundle(control, plans, bound_item)?;
        let environment = DockerExecutionEnvironmentV1::new(self.config.image.clone(), Vec::new())
            .map_err(domain)?;
        let environment_bytes = environment.to_bytes().map_err(domain)?;
        let input_id = ContentId::<InputBundleArtifact>::derive(&bundle_bytes).map_err(domain)?;
        let environment_id = ContentId::<ExecutionEnvironmentArtifact>::derive(&environment_bytes)
            .map_err(domain)?;
        let job_id = JobId::new();
        let contract = JobContract::new(
            job_id,
            input_id,
            environment_id,
            ExecutionBackend::new(DOCKER_BACKEND).map_err(domain)?,
            CommandContract::new(
                SandboxPath::new("bin/run").map_err(domain)?,
                Vec::new(),
                SandboxPath::new("work").map_err(domain)?,
            ),
            ResourceRequest::new(
                self.config.execution_timeout,
                PlacementRequest::new(
                    cairn_execution::ExecutionPlatformRequirement::default(),
                    vec![self.config.worker_pool.clone()],
                    Vec::new(),
                )
                .map_err(domain)?,
            )
            .map_err(domain)?,
            NetworkPolicy::Disabled,
            CapturePolicy::new(
                OutputByteLimit::new(16 * 1024).map_err(domain)?,
                OutputByteLimit::new(16 * 1024).map_err(domain)?,
                DiagnosticByteLimit::new(4 * 1024).map_err(domain)?,
                EvidenceByteLimit::new(64 * 1024).map_err(domain)?,
                Vec::new(),
            )
            .map_err(domain)?,
        );
        {
            let mut content = self.content()?;
            archive_exact(&mut content, input_id, &bundle)?;
            archive_bytes(&mut content, environment_id, &environment_bytes)?;
        }
        let deadline =
            tokio::time::Instant::now() + Duration::from_millis(self.config.completion_timeout_ms);
        let scheduling_retry_interval =
            Duration::from_millis(self.config.poll_interval_ms).max(MIN_SCHEDULING_RETRY_INTERVAL);
        let (attempt_id, reservation_id) = loop {
            let attempt_id = AttemptId::new();
            let reservation_id = ReservationId::new();
            let ids = ControllerScheduleIds {
                attempt_id,
                placement_id: PlacementId::new(),
                reservation_id,
                assignment_id: AssignmentId::new(),
                lease_id: LeaseId::new(),
                offer_message_id: ControlMessageId::new(),
                start_message_id: ControlMessageId::new(),
                commands: ControllerScheduleCommandIds {
                    authorize_attempt: CommandId::new(),
                    reserve_placement: CommandId::new(),
                    grant_assignment: CommandId::new(),
                    enqueue_offer: CommandId::new(),
                },
            };
            match tokio::task::block_in_place(|| {
                schedule_execution_contract(&self.server, &contract, ids)
            })
            .map_err(domain)?
            {
                ControllerSchedulingOutcome::Scheduled { .. } => {
                    break (attempt_id, reservation_id);
                }
                ControllerSchedulingOutcome::NoCandidate { .. } => {
                    if tokio::time::Instant::now() >= deadline {
                        return Err(OracleControlRunnerError::NoWorker);
                    }
                    tokio::time::sleep(scheduling_retry_interval).await;
                }
            }
        };
        let contract_id =
            ContentId::derive(&cairn_codec::to_vec(&contract).map_err(domain)?).map_err(domain)?;
        let job = ExecutionJob::new(job_id).map_err(domain)?;
        loop {
            let events = SqliteEventStore::open(self.server.event_database()).map_err(domain)?;
            let content = self.content()?;
            match recover_execution_job(&events, &content, &job).map_err(domain)? {
                ExecutionJobState::Completed {
                    receipt_id,
                    receipt,
                } => {
                    let release_reason = release_execution_reservation(
                        &self.server,
                        reservation_id,
                        &CommandId::new(),
                    )
                    .map_err(domain)?;
                    if release_reason != ReservationReleaseReason::ExecutionTerminal {
                        return Err(OracleControlRunnerError::Binding);
                    }
                    let result = if receipt.outcome() == ExecutionOutcome::Succeeded
                        && receipt.exit_code() == Some(0)
                    {
                        OracleControlResultV1::Passed
                    } else {
                        OracleControlResultV1::Failed
                    };
                    return Ok(WorkerReceiptV1 {
                        job_id,
                        attempt_id,
                        contract_id,
                        receipt_id,
                        receipt,
                        result,
                    });
                }
                ExecutionJobState::NotStarted { .. } | ExecutionJobState::Ambiguous { .. } => {
                    return Err(OracleControlRunnerError::WorkerRejected);
                }
                ExecutionJobState::NotFound
                | ExecutionJobState::ReadyToStart(_)
                | ExecutionJobState::InDoubt { .. } => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(OracleControlRunnerError::WorkerTimeout);
            }
            tokio::time::sleep(Duration::from_millis(self.config.poll_interval_ms)).await;
        }
    }

    fn content(&self) -> Result<SqliteContentStore, OracleControlRunnerError> {
        SqliteContentStore::open(
            self.server.content_database(),
            self.server.content_directory(),
        )
        .map_err(domain)
    }
}

fn candidate_facing_runner_available() -> bool {
    false
}

fn classify_control_failure(
    control: OracleControlFamilyV1,
    result: OracleControlResultV1,
    outcome: ExecutionOutcome,
    exit_code: Option<i32>,
) -> Option<OracleControlFailureClassV1> {
    if result == OracleControlResultV1::Passed {
        return None;
    }
    match (outcome, control, exit_code) {
        (ExecutionOutcome::SubjectFailed, OracleControlFamilyV1::Honest, Some(1)) => {
            Some(OracleControlFailureClassV1::OracleArtifactRejected)
        }
        (
            ExecutionOutcome::SubjectFailed,
            OracleControlFamilyV1::Mutant
            | OracleControlFamilyV1::Hidden
            | OracleControlFamilyV1::Bypass,
            Some(31),
        ) => Some(OracleControlFailureClassV1::NegativeChallengeAccepted),
        (ExecutionOutcome::SubjectFailed, _, Some(32)) => {
            Some(OracleControlFailureClassV1::MechanismProtocolViolation)
        }
        _ => Some(OracleControlFailureClassV1::ExecutionFailure),
    }
}

fn validate_plan_coverage(
    proposal: &OraclePortfolioProposalV1,
    plans: &[OracleCheckPlanV1],
) -> Result<(), OracleControlRunnerError> {
    if proposal.entries().iter().any(|entry| {
        !matches!(
            entry.resolution(),
            OracleObligationResolutionV1::Contributed { .. }
        )
    }) {
        return Err(OracleControlRunnerError::Binding);
    }
    let mut expected = proposal
        .accepted_items()
        .iter()
        .flat_map(cairn_migration::OracleAcceptedItemV1::plans)
        .map(|plan| plan.identity().map(|identity| identity.to_wire()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(domain)?;
    let mut actual = plans
        .iter()
        .map(|plan| plan.identity().map(|identity| identity.to_wire()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(domain)?;
    expected.sort();
    actual.sort();
    if expected.is_empty() || expected != actual {
        return Err(OracleControlRunnerError::Binding);
    }
    Ok(())
}

fn build_bundle(
    control: OracleControlFamilyV1,
    plans: &[OracleCheckPlanV1],
    bound_item: Option<&OracleItemV1>,
) -> Result<(InputBundleV1, Vec<u8>), OracleControlRunnerError> {
    let mut entries = vec![
        directory("bin")?,
        directory("meta")?,
        directory("plans")?,
        directory("work")?,
        file("bin/run", InputFileMode::Executable, RUNNER.to_vec())?,
        file(
            "meta/control-mode",
            InputFileMode::Data,
            format!("{}\n", control_name(control)).into_bytes(),
        )?,
    ];
    let mut index = String::new();
    for (ordinal, plan) in plans.iter().enumerate() {
        let original = cairn_codec::to_vec(plan).map_err(domain)?;
        let item = plan.item().to_wire();
        let (bytes, expected_item) = match control {
            OracleControlFamilyV1::MechanismQualification | OracleControlFamilyV1::Honest => {
                (original, item)
            }
            OracleControlFamilyV1::Mutant => {
                let mut value = serde_json::to_value(plan).map_err(domain)?;
                value["pass_condition"] = serde_json::Value::String(String::new());
                (serde_json::to_vec(&value).map_err(domain)?, item)
            }
            OracleControlFamilyV1::Hidden => {
                let challenge = hidden_binding_challenge(
                    plan,
                    bound_item.ok_or(OracleControlRunnerError::Binding)?,
                )?;
                if challenge == plan.item() {
                    return Err(OracleControlRunnerError::Binding);
                }
                (original, challenge.to_wire())
            }
            OracleControlFamilyV1::Bypass => {
                let mut value = serde_json::to_value(plan).map_err(domain)?;
                value["schema_version"] = serde_json::Value::from(2);
                value["unexpected"] = serde_json::Value::Bool(true);
                (serde_json::to_vec(&value).map_err(domain)?, item)
            }
        };
        let path = format!("plans/{ordinal}.json");
        index.push_str(&format!("{path}|{expected_item}|{}\n", sha256_hex(&bytes)));
        entries.push(file(&path, InputFileMode::Data, bytes)?);
    }
    entries.push(file(
        "meta/control-index",
        InputFileMode::Data,
        index.into_bytes(),
    )?);
    let bundle = InputBundleV1::new(entries).map_err(domain)?;
    let bytes = bundle.to_bytes().map_err(domain)?;
    Ok((bundle, bytes))
}

fn hidden_binding_challenge(
    plan: &OracleCheckPlanV1,
    item: &OracleItemV1,
) -> Result<ContentId<OracleItemArtifact>, OracleControlRunnerError> {
    let plan_id = plan.identity().map_err(domain)?;
    if item.identity().map_err(domain)? != plan.item() {
        return Err(OracleControlRunnerError::Binding);
    }
    OracleItemV1::new(
        item.dimension(),
        OracleItemStatement::new(format!(
            "Control-only hidden binding challenge for exact plan {plan_id}."
        ))
        .map_err(domain)?,
    )
    .and_then(|challenge| challenge.identity())
    .map_err(domain)
}

fn control_name(control: OracleControlFamilyV1) -> &'static str {
    match control {
        OracleControlFamilyV1::MechanismQualification => "mechanism-qualification",
        OracleControlFamilyV1::Honest => "honest",
        OracleControlFamilyV1::Mutant => "mutant",
        OracleControlFamilyV1::Hidden => "hidden",
        OracleControlFamilyV1::Bypass => "bypass",
    }
}

fn directory(path: &str) -> Result<InputBundleEntry, OracleControlRunnerError> {
    Ok(InputBundleEntry::Directory {
        path: SandboxPath::new(path).map_err(domain)?,
    })
}

fn file(
    path: &str,
    mode: InputFileMode,
    bytes: Vec<u8>,
) -> Result<InputBundleEntry, OracleControlRunnerError> {
    Ok(InputBundleEntry::File {
        path: SandboxPath::new(path).map_err(domain)?,
        mode,
        bytes,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to String cannot fail");
            output
        })
}

fn derive<T: ContentType>(
    value: &impl Serialize,
) -> Result<ContentId<T>, OracleControlRunnerError> {
    ContentId::derive(&cairn_codec::to_vec(value).map_err(domain)?).map_err(domain)
}

fn archive_exact<T: ContentType>(
    content: &mut SqliteContentStore,
    expected: ContentId<T>,
    value: &impl Serialize,
) -> Result<(), OracleControlRunnerError> {
    archive_bytes(
        content,
        expected,
        &cairn_codec::to_vec(value).map_err(domain)?,
    )
}

fn archive_bytes<T: ContentType>(
    content: &mut SqliteContentStore,
    expected: ContentId<T>,
    bytes: &[u8],
) -> Result<(), OracleControlRunnerError> {
    let actual = content
        .put::<T>(&mut Cursor::new(bytes))
        .map_err(domain)?
        .content_id;
    if actual == expected {
        Ok(())
    } else {
        Err(OracleControlRunnerError::Binding)
    }
}

fn domain(error: impl std::fmt::Display) -> OracleControlRunnerError {
    OracleControlRunnerError::Domain(error.to_string())
}

#[derive(Debug, Error)]
pub enum OracleControlRunnerError {
    #[error("Oracle control Worker configuration is invalid")]
    InvalidConfiguration,
    #[error("no candidate-facing executable Oracle mechanism is available")]
    SemanticExecutionUnavailable,
    #[error("Oracle control binding changed")]
    Binding,
    #[error("Oracle controls have not been qualified for this task")]
    NotQualified,
    #[error("no Worker satisfies the Oracle control placement")]
    NoWorker,
    #[error("Oracle control Worker rejected or ambiguously completed the job")]
    WorkerRejected,
    #[error("Oracle control Worker completion timed out")]
    WorkerTimeout,
    #[error("Oracle control runner failed at a domain boundary: {0}")]
    Domain(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_migration::{
        MigrationIntentContractArtifact, OracleCheckEvidenceV1, OracleCheckMethodV1,
        OracleCheckObjective, OracleCheckObservation, OracleCheckPassCondition, OracleCheckSetup,
        OracleDimensionArtifact,
    };

    fn item() -> OracleItemV1 {
        OracleItemV1::new(
            ContentId::<OracleDimensionArtifact>::derive(b"generic-dimension").expect("dimension"),
            OracleItemStatement::new("Check one task-local observable property.")
                .expect("statement"),
        )
        .expect("item")
    }

    fn plan(item: &OracleItemV1) -> OracleCheckPlanV1 {
        OracleCheckPlanV1::new(
            item.identity().expect("item identity"),
            OracleCheckMethodV1::StaticAnalysis,
            OracleCheckObjective::new("Inspect the offered implementation.").expect("objective"),
            OracleCheckSetup::new("Open the cited task-local source.").expect("setup"),
            OracleCheckObservation::new("Record the implementation property.")
                .expect("observation"),
            OracleCheckPassCondition::new("The property matches the admitted intent.")
                .expect("pass condition"),
            vec![OracleCheckEvidenceV1::AdmittedIntent {
                contract: ContentId::<MigrationIntentContractArtifact>::derive(b"intent")
                    .expect("intent"),
            }],
        )
        .expect("plan")
    }

    #[test]
    fn hidden_control_uses_a_distinct_deterministic_typed_item_challenge() {
        let item = item();
        let plan = plan(&item);
        let first = hidden_binding_challenge(&plan, &item).expect("challenge");
        let second = hidden_binding_challenge(&plan, &item).expect("same challenge");

        assert_eq!(first, second);
        assert_ne!(first, plan.item());
    }

    #[test]
    fn structural_plan_validator_cannot_grant_semantic_qualification() {
        assert!(!candidate_facing_runner_available());
        assert_eq!(
            OracleControlRunnerError::SemanticExecutionUnavailable.to_string(),
            "no candidate-facing executable Oracle mechanism is available"
        );
    }

    #[test]
    fn runner_exit_semantics_route_artifact_and_mechanism_failures_separately() {
        assert_eq!(
            classify_control_failure(
                OracleControlFamilyV1::Honest,
                OracleControlResultV1::Failed,
                ExecutionOutcome::SubjectFailed,
                Some(1),
            ),
            Some(OracleControlFailureClassV1::OracleArtifactRejected)
        );
        assert_eq!(
            classify_control_failure(
                OracleControlFamilyV1::Hidden,
                OracleControlResultV1::Failed,
                ExecutionOutcome::SubjectFailed,
                Some(31),
            ),
            Some(OracleControlFailureClassV1::NegativeChallengeAccepted)
        );
        assert_eq!(
            classify_control_failure(
                OracleControlFamilyV1::Bypass,
                OracleControlResultV1::Failed,
                ExecutionOutcome::SubjectFailed,
                Some(32),
            ),
            Some(OracleControlFailureClassV1::MechanismProtocolViolation)
        );
        assert_eq!(
            classify_control_failure(
                OracleControlFamilyV1::Mutant,
                OracleControlResultV1::Failed,
                ExecutionOutcome::InfrastructureFailed,
                None,
            ),
            Some(OracleControlFailureClassV1::ExecutionFailure)
        );
    }
}
