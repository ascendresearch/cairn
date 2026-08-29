use std::{
    fs,
    io::{Cursor, Write},
    path::Path,
    process::{Command, Stdio},
    str::FromStr,
};

use cairn_admission::{
    AuthoritativeIntentClaimV1, CollectionOutputIntentV1, CollectionOutputOrderContractV1,
    TaskIntentAuthoritySubject, UserIntentAuthorityGrantArtifact, UserIntentAuthorityGrantV1,
    UserIntentAuthorityScopeV1, UserIntentDecisionArtifact, UserIntentDecisionResponseV1,
    UserIntentDecisionV1, admit_collection_oracle_claim, commit_collection_oracle_admission,
    derive_collection_output_oracle_decision, prepare_collection_candidate_search_input,
    promote_user_intent,
};
use cairn_execution::{
    CapturedOutput, DiagnosticByteLimit, EvidenceByteLimit, ExecutionBackend, ExecutionCapture,
    ExecutionCompletion, ExecutionElapsedMillis, ExecutionEnvironmentArtifact, ExecutionInput,
    ExecutionOutcome, InputBundleArtifact, OutputByteLimit, ResolvedProgramIdentity,
    ScriptedExecutor, TrustedExecutionEvidence, authorize_execution_attempt,
    begin_execution_attempt, execute_execution_attempt, prepare_execution_job,
};
use cairn_migration::{
    AssembledCollectionF32OracleCaseInput, CallAdapterCaptureLimits, CallAdapterCompletionV1,
    CallAdapterExecutableByteLimit, CallAdapterObservedOutputV1, CallAdapterResultV1,
    CollectionF32Bits, CollectionOracleElementArtifact, CollectionOracleQualificationExecution,
    CollectionOutputComparisonV1, CollectionReportedCount, ExpectedCollectionOracleOutputV1,
    IntentHypothesisSetProposalV1, IntentRecoveryInputArtifact, IntentRecoveryInputV1,
    MigrationExecutionNeed, MigrationValidationTier, ObservedCollectionOracleOutputV1,
    PreparedCallAdapterInput, PreparedCallAdapterJob, SirCallerClaimId, SirHypothesisId,
    SirIntentHypothesisSetProposalArtifact, SirProcessRequestV1, SirProcessTerminalV1,
    SirTaskBundleArtifact, SirTaskBundleV1, UserIntentDecisionRequestArtifact,
    ValidatedCallAdapterExecution, assemble_collection_f32_oracle_case, compose_call_adapter_job,
    derive_user_intent_decision_requests, prepare_collection_oracle_claim_proposal,
    prepare_collection_output_call_adapter_input, validate_collection_output_call_adapter_receipt,
};
use cairn_protocol::{
    AttemptId, CommandId, ContentId, ContentType, JobId, ObservedAtUnixMillis, OperationId,
    SirRunId,
};
use cairn_record::ContentStore;
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde::de::DeserializeOwned;

#[test]
#[ignore = "requires the private DEV-006 live run store; makes no provider call"]
#[allow(clippy::too_many_lines)]
fn exact_live_proposal_crosses_process_and_drives_first_admitted_oracle_policy() {
    let run_root = std::env::var("CAIRN_DEV006_RUN_ROOT").expect("DEV-006 run root");
    let proposal_id = ContentId::<SirIntentHypothesisSetProposalArtifact>::from_str(
        &std::env::var("CAIRN_DEV006_PROPOSAL_ID").expect("DEV-006 proposal ID"),
    )
    .expect("typed proposal ID");
    let store = SqliteContentStore::open_immutable_read_only(
        Path::new(&run_root).join("content.db"),
        Path::new(&run_root).join("cas"),
    )
    .expect("live run store");
    let proposal: IntentHypothesisSetProposalV1 = load(&store, &proposal_id);
    let recovery_input_id = proposal.recovery_input();
    let recovery_input: IntentRecoveryInputV1 = load(&store, &recovery_input_id);
    let task_bundle: SirTaskBundleV1 =
        load::<SirTaskBundleArtifact, _>(&store, &recovery_input.task_bundle());
    let process_request = SirProcessRequestV1::new(
        SirRunId::new(),
        OperationId::new(),
        task_bundle.clone(),
        recovery_input.clone(),
        proposal.clone(),
    )
    .expect("exact SIR process request");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cairn-sir"))
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("isolated SIR process");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(&cairn_codec::to_vec(&process_request).expect("request bytes"))
        .expect("write request");
    let output = child.wait_with_output().expect("terminal output");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let terminal: SirProcessTerminalV1 =
        cairn_codec::from_slice(&output.stdout).expect("strict process terminal");
    assert_eq!(terminal.proposal_id(), proposal_id);

    let batch = derive_user_intent_decision_requests(
        proposal_id,
        terminal.proposal(),
        recovery_input_id,
        &recovery_input,
    )
    .expect("exact live decision request");
    assert_eq!(batch.requests().len(), 1);
    let request = &batch.requests()[0];
    let request_id = request.identity().expect("request identity");
    let chosen =
        SirHypothesisId::new("h-compact-set-order-unspecified").expect("chosen hypothesis");
    assert!(
        request
            .options()
            .iter()
            .any(|option| option.hypothesis() == &chosen)
    );
    let selection_claim = SirCallerClaimId::new("copies-strictly-above").expect("selection claim");
    let grant = UserIntentAuthorityGrantV1::new(
        recovery_input.task_id(),
        TaskIntentAuthoritySubject::new("task-authority:user").expect("authority subject"),
        UserIntentAuthorityScopeV1::CollectionOutput {
            selection_claim: selection_claim.clone(),
        },
    );
    let grant_id = grant.identity().expect("grant identity");
    let decision = UserIntentDecisionV1::new(
        request_id,
        grant_id,
        UserIntentDecisionResponseV1::SelectHypothesis {
            hypothesis: chosen,
            authoritative_claim: AuthoritativeIntentClaimV1::CollectionOutput(
                CollectionOutputIntentV1::exact_selected_occurrences(
                    selection_claim,
                    CollectionOutputOrderContractV1::UnspecifiedPermutation,
                ),
            ),
        },
    );
    let decision_id = decision.identity().expect("decision identity");
    if let Ok(smoke_root) = std::env::var("CAIRN_DEV008_SMOKE_ROOT") {
        export_principal_smoke_state(
            Path::new(&smoke_root),
            &process_request,
            &task_bundle,
            &recovery_input,
            proposal_id,
            &proposal,
            request,
            &grant,
            &decision,
        );
    }
    let prepared = promote_user_intent(
        proposal_id,
        terminal.proposal(),
        recovery_input_id,
        &recovery_input,
        request_id,
        request,
        grant_id,
        &grant,
        decision_id,
        &decision,
    )
    .expect("first exact promotion");
    let oracle = derive_collection_output_oracle_decision(prepared.public_outcome())
        .expect("contract-only Oracle decision");
    let local_oracle_proposal =
        prepare_collection_oracle_claim_proposal(&oracle).expect("local Oracle claim proposal");
    assert_eq!(local_oracle_proposal.contract(), oracle.contract());
    assert_eq!(
        local_oracle_proposal.selection_claim(),
        oracle.selection_claim()
    );
    let first = ContentId::<CollectionOracleElementArtifact>::derive(b"selected-a").expect("first");
    let second =
        ContentId::<CollectionOracleElementArtifact>::derive(b"selected-b").expect("second");
    let expected = ExpectedCollectionOracleOutputV1::new(vec![first, second]).expect("expected");
    let reordered =
        ObservedCollectionOracleOutputV1::new(vec![second, first], CollectionReportedCount::new(2))
            .expect("reordered");
    assert_eq!(
        oracle.compare(&expected, &reordered),
        CollectionOutputComparisonV1::Equivalent
    );

    let f32_input = [1.0_f32, 4.0, 3.0, 2.0]
        .into_iter()
        .map(|value| CollectionF32Bits::new(value.to_bits()).expect("normal f32"))
        .collect::<Vec<_>>();
    let f32_case = assemble_collection_f32_oracle_case(
        &oracle,
        &f32_input,
        CollectionF32Bits::new(2.0_f32.to_bits()).expect("threshold"),
    )
    .expect("exact contract-bound materializer");
    let reversed_actual = ObservedCollectionOracleOutputV1::new(
        [3.0_f32, 4.0]
            .into_iter()
            .map(|value| {
                ContentId::<CollectionOracleElementArtifact>::derive(&value.to_bits().to_le_bytes())
                    .expect("observed f32 element")
            })
            .collect(),
        CollectionReportedCount::new(2),
    )
    .expect("reversed actual output");
    assert_eq!(
        oracle.compare(f32_case.expected(), &reversed_actual),
        CollectionOutputComparisonV1::Equivalent
    );
    if let Ok(export_root) = std::env::var("CAIRN_DEV012_EXPORT_ROOT") {
        export_candidate_input(
            Path::new(&export_root),
            prepared.public_outcome(),
            &recovery_input,
            &f32_case,
        );
    }
}

const EXPORT_BACKEND: &str = "dev012-exact-candidate-input-v1";

struct CompletedControl {
    _directory: tempfile::TempDir,
    content: SqliteContentStore,
    adapter: PreparedCallAdapterInput,
    execution: ValidatedCallAdapterExecution,
}

fn export_candidate_input(
    root: &Path,
    intent: &cairn_admission::IntentAdmissionPublicOutcomeV1,
    recovery_input: &IntentRecoveryInputV1,
    case: &AssembledCollectionF32OracleCaseInput,
) {
    fs::create_dir_all(root).expect("DEV-012 export root");
    let honest = complete_control(root, case, b"exact honest implementation", &[3.0, 4.0]);
    let fault = complete_control(root, case, b"exact missing implementation", &[3.0]);
    let prepared = admit_collection_oracle_claim(
        intent,
        case,
        &CollectionOracleQualificationExecution {
            adapter_input: &honest.adapter,
            execution: &honest.execution,
            content: &honest.content,
        },
        &CollectionOracleQualificationExecution {
            adapter_input: &fault.adapter,
            execution: &fault.execution,
            content: &fault.content,
        },
    )
    .expect("exact local Oracle admission");
    let restricted_root = root.join("restricted");
    fs::create_dir_all(&restricted_root).expect("restricted export root");
    let mut restricted = SqliteContentStore::open(
        restricted_root.join("content.db"),
        restricted_root.join("cas"),
    )
    .expect("restricted export store");
    let published = commit_collection_oracle_admission(&mut restricted, &prepared)
        .expect("restricted commit before Candidate publication");
    let candidate = prepare_collection_candidate_search_input(&published)
        .expect("exact Candidate search input");
    fs::write(
        root.join("recovery-input.json"),
        cairn_codec::to_vec(recovery_input).expect("recovery bytes"),
    )
    .expect("write recovery input");
    fs::write(
        root.join("oracle-public-outcome.json"),
        cairn_codec::to_vec(&published).expect("public outcome bytes"),
    )
    .expect("write public outcome");
    fs::write(root.join("candidate-search-input.json"), candidate.bytes())
        .expect("write Candidate search input");
    fs::write(
        root.join("candidate-search-input-id"),
        candidate.id().to_wire(),
    )
    .expect("write Candidate search input ID");
}

fn complete_control(
    export_root: &Path,
    case: &AssembledCollectionF32OracleCaseInput,
    executable: &[u8],
    selected: &[f32],
) -> CompletedControl {
    let adapter = prepare_collection_output_call_adapter_input(
        case,
        executable,
        CallAdapterExecutableByteLimit::new(
            u64::try_from(executable.len()).expect("executable length"),
        )
        .expect("executable limit"),
    )
    .expect("adapter input");
    let directory = tempfile::tempdir_in(export_root).expect("control state");
    let mut content = SqliteContentStore::open(
        directory.path().join("content.db"),
        directory.path().join("cas"),
    )
    .expect("control content store");
    let mut events =
        SqliteEventStore::open(directory.path().join("events.db")).expect("control event store");
    assert_eq!(
        content
            .put::<InputBundleArtifact>(&mut Cursor::new(adapter.input_bundle_bytes()))
            .expect("input bundle")
            .content_id,
        adapter.input_bundle_id()
    );
    let environment = content
        .put::<ExecutionEnvironmentArtifact>(&mut Cursor::new(b"host environment"))
        .expect("environment")
        .content_id;
    let need = MigrationExecutionNeed::new(
        MigrationValidationTier::V0Cpu,
        ExecutionBackend::new(EXPORT_BACKEND).expect("backend"),
        cairn_execution::ExecutionTimeoutMillis::new(5_000).expect("timeout"),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
    )
    .expect("execution need");
    let job = compose_call_adapter_job(
        JobId::new(),
        &adapter,
        environment,
        &need,
        CallAdapterCaptureLimits {
            stdout: OutputByteLimit::new(1_024).expect("stdout"),
            stderr: OutputByteLimit::new(1_024).expect("stderr"),
            result: OutputByteLimit::new(4_096).expect("result"),
            diagnostic: DiagnosticByteLimit::new(1_024).expect("diagnostic"),
            evidence: EvidenceByteLimit::new(4_096).expect("evidence"),
        },
    )
    .expect("adapter job");
    let prepared = prepare_execution_job(&mut content, job.contract()).expect("prepared job");
    let authority = authorize_execution_attempt(
        &mut events,
        prepared,
        AttemptId::new(),
        &CommandId::new(),
        ObservedAtUnixMillis::new(1),
    )
    .expect("execution authority");
    let started = begin_execution_attempt(
        &mut events,
        authority,
        &CommandId::new(),
        ObservedAtUnixMillis::new(2),
    )
    .expect("started execution");
    let capture = synthetic_capture(case, &adapter, &job, environment, selected);
    let mut executor = ScriptedExecutor::new(move |_: &ExecutionInput<'_>| Ok(capture.clone()));
    let ExecutionCompletion::Completed {
        receipt_id,
        receipt,
    } = execute_execution_attempt(
        &mut events,
        &mut content,
        &mut executor,
        started,
        &CommandId::new(),
        ObservedAtUnixMillis::new(3),
    )
    .expect("execution completion")
    else {
        panic!("expected completed control execution");
    };
    let execution = validate_collection_output_call_adapter_receipt(
        case, &adapter, &job, receipt_id, &receipt, &content,
    )
    .expect("validated control receipt");
    CompletedControl {
        _directory: directory,
        content,
        adapter,
        execution,
    }
}

fn synthetic_capture(
    case: &AssembledCollectionF32OracleCaseInput,
    adapter: &PreparedCallAdapterInput,
    job: &PreparedCallAdapterJob,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    selected: &[f32],
) -> ExecutionCapture {
    let mut values = vec![
        0_u8;
        usize::try_from(case.invocation().values_output().byte_length().get())
            .expect("values capacity")
    ];
    for (destination, value) in values.chunks_exact_mut(4).zip(selected) {
        destination.copy_from_slice(&value.to_bits().to_le_bytes());
    }
    let count = u32::try_from(selected.len())
        .expect("selected count")
        .to_le_bytes()
        .to_vec();
    let output_bytes = [values, count];
    let mut observed = adapter
        .request()
        .expected_outputs()
        .iter()
        .zip(&output_bytes)
        .map(|(expected, bytes)| {
            CallAdapterObservedOutputV1::from_bytes(
                expected.argument_index(),
                expected.buffer().clone(),
                bytes,
            )
            .expect("observed ABI output")
        })
        .collect::<Vec<_>>();
    observed.sort_by_key(CallAdapterObservedOutputV1::argument_index);
    let result = CallAdapterResultV1::new(
        adapter.request_id(),
        adapter.request().invocation(),
        CallAdapterCompletionV1::InvokedVoid,
        observed,
    )
    .expect("adapter result");
    let result_bytes = cairn_codec::to_vec(&result).expect("result bytes");
    let captured = job
        .contract()
        .capture()
        .expected_outputs()
        .iter()
        .map(|declared| {
            let bytes = if declared.path == *adapter.request().result_path() {
                result_bytes.clone()
            } else {
                let position = adapter
                    .request()
                    .expected_outputs()
                    .iter()
                    .position(|expected| expected.path() == &declared.path)
                    .expect("declared ABI output");
                output_bytes[position].clone()
            };
            CapturedOutput {
                name: declared.name.clone(),
                bytes,
            }
        })
        .collect::<Vec<_>>();
    let evidence = TrustedExecutionEvidence::new(
        ExecutionBackend::new(EXPORT_BACKEND).expect("backend"),
        environment,
        ResolvedProgramIdentity::new(adapter.request().executable().to_wire())
            .expect("program identity"),
        Vec::new(),
    )
    .expect("execution evidence");
    ExecutionCapture::new(
        ExecutionOutcome::Succeeded,
        Some(0),
        ExecutionElapsedMillis::new(1),
        Vec::new(),
        Vec::new(),
        captured,
        evidence,
    )
}

#[allow(clippy::too_many_arguments)]
fn export_principal_smoke_state(
    root: &Path,
    process_request: &SirProcessRequestV1,
    task_bundle: &SirTaskBundleV1,
    recovery_input: &IntentRecoveryInputV1,
    proposal_id: ContentId<SirIntentHypothesisSetProposalArtifact>,
    proposal: &IntentHypothesisSetProposalV1,
    request: &cairn_migration::UserIntentDecisionRequestV1,
    grant: &UserIntentAuthorityGrantV1,
    decision: &UserIntentDecisionV1,
) {
    fs::create_dir_all(root).expect("smoke root");
    fs::write(
        root.join("sir-request.json"),
        cairn_codec::to_vec(process_request).expect("process request bytes"),
    )
    .expect("write process request");
    let public = root.join("public");
    fs::create_dir_all(&public).expect("public Controller root");
    let mut store = SqliteContentStore::open(public.join("content.db"), public.join("cas"))
        .expect("public Controller store");
    archive::<SirTaskBundleArtifact>(&mut store, task_bundle);
    archive::<IntentRecoveryInputArtifact>(&mut store, recovery_input);
    assert_eq!(
        archive::<SirIntentHypothesisSetProposalArtifact>(&mut store, proposal),
        proposal_id
    );
    archive::<UserIntentDecisionRequestArtifact>(&mut store, request);
    archive::<UserIntentAuthorityGrantArtifact>(&mut store, grant);
    let decision_id = archive::<UserIntentDecisionArtifact>(&mut store, decision);
    fs::write(root.join("decision-id"), decision_id.to_wire()).expect("write decision ID");
}

fn archive<T: ContentType>(
    store: &mut SqliteContentStore,
    value: &impl serde::Serialize,
) -> ContentId<T> {
    store
        .put::<T>(&mut Cursor::new(
            cairn_codec::to_vec(value).expect("canonical archive bytes"),
        ))
        .expect("archive content")
        .content_id
}

fn load<T, V>(store: &SqliteContentStore, content_id: &ContentId<T>) -> V
where
    T: ContentType,
    V: DeserializeOwned,
{
    let mut bytes = Vec::new();
    store
        .write_to(content_id, &mut bytes)
        .expect("content bytes");
    cairn_codec::from_slice(&bytes).expect("strict typed content")
}
