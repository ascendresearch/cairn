use std::{fs, path::Path};

use cairn_agent::{
    AdapterVersion, DeploymentName, EpisodeBudget, EpisodeStepLimit, EpisodeToolOperationLimit,
    ModelName, ModelOutputTokenLimit, ModelProtocolConfig, ModelSelection, ModelTransportResponse,
    ProviderName, ResponsesReasoningReplay, ScriptedModelTransport, ToolEffectClass,
    TransportError,
};
use cairn_execution::{
    ExecutionEvidenceArtifact, ExecutionReceipt, ExecutionReceiptArtifact, ExecutionStderrArtifact,
    ExecutionStdoutArtifact, JobContractArtifact,
};
use cairn_migration::{
    AgentResolvedRuntimeModelArtifact, AuthoritativeIntentClaimV1, CandidateOracleContractV1,
    CandidateOracleElementMaterialV1, CandidateOracleMaterialV1, CandidateOracleMaterialsV1,
    CandidateWorkspaceV1, CollectionOutputIntentV1, CollectionOutputOrderContractV1,
    MigrationIntentContractArtifact, OracleAdmissionOutcomeArtifact, OracleAdversarialPolicyV1,
    OracleBuildTestSnapshotArtifact, OracleClaimName, OracleClaimV1, OracleConcernV1,
    OracleCoveragePolicyV1, OracleCoverageProfileV1, OracleDocumentationSnapshotArtifact,
    OracleExperimentLimit, OracleExperimentToolCatalogArtifact, OracleExplorationBudgetV1,
    OracleExplorationCapabilityGrantArtifact, OracleKnowledgeSnapshotArtifact,
    OraclePortfolioElementKindV1, OraclePortfolioElementV1, OraclePortfolioProposalArtifact,
    OracleResearchToolCatalogArtifact, OracleSourceSnapshotArtifact, OracleStrategyCatalogV1,
    OracleStrategyExecutorV1, OracleStrategyKindV1, OracleStrategyName,
    OracleStrategyRegistrationV1, OracleStrategyRoleV1, OracleStrategyRunArtifact,
    OracleStrategyRunLimit, OracleStrategyRunV1, OracleStrategyToolCatalogV1,
    OracleWorkspaceArtifact, OracleWorkspaceInput, OracleWorkspaceV1, ProposalHostBinaryIdentity,
    ProposalHostExperimentDispatchV1, ProposalHostExperimentOperationV1,
    ProposalHostExperimentWorker, ProposalHostExperimentWorkerError,
    ProposalHostOracleBuildTestsV1, ProposalHostOracleDocumentationV1,
    ProposalHostOracleKnowledgeV1, ProposalHostOracleMaterialsV1, ProposalHostPublicationV1,
    ProposalHostRequestV1, ProposalHostRoleRequestV1, ProposalHostRuntimeV1,
    ProposalHostTaskSnapshotV1, ProposalHostWorkerBindingV1, ProposalHostWorkerObservationV1,
    SirCallerClaimId, SirTaskLimits, SirTaskWorkspace, derive_oracle_claims,
    derive_oracle_work_items, execute_proposal_host_experiments, run_proposal_host_episode,
};
use cairn_protocol::{AttemptId, ContentId, ContentType, EpisodeId, JobId, TaskId};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use cairn_verification::ModelConfigurationArtifact;
use serde_json::json;

fn id<T: ContentType>(label: &[u8]) -> ContentId<T> {
    ContentId::derive(label).expect("content identity")
}

fn codec() -> cairn_agent::NativeProtocolCodec {
    cairn_agent::NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiResponses {
        store: false,
        reasoning_replay: ResponsesReasoningReplay::PreserveOutputItems,
    })
    .expect("native codec")
}

fn runtime(episode_id: EpisodeId, label: &[u8]) -> ProposalHostRuntimeV1 {
    ProposalHostRuntimeV1::new(
        episode_id,
        ProposalHostBinaryIdentity::new(format!("sha256:{}", "1".repeat(64)))
            .expect("Host binary identity"),
        id::<AgentResolvedRuntimeModelArtifact>(label),
        ModelSelection {
            provider: ProviderName::new("recorded").expect("provider"),
            model: ModelName::new("recorded-role-model").expect("model"),
            deployment: DeploymentName::new("isolated-recorded").expect("deployment"),
            adapter_version: AdapterVersion::new("native-protocol-v1").expect("adapter"),
        },
        EpisodeBudget {
            step_limit: Some(EpisodeStepLimit::new(6).expect("steps")),
            tool_operation_limit: Some(EpisodeToolOperationLimit::new(12)),
            provider_token_limit: None,
            deadline_unix_ms: None,
            external_meter_limits: None,
        },
        ModelOutputTokenLimit::new(16_384).expect("output limit"),
        SirTaskLimits::default(),
    )
}

fn write_candidate_task(root: &Path) {
    fs::write(
        root.join("select.cu"),
        "__global__ void scatter(const float* input, const unsigned* index, unsigned count, float* output) {\n  unsigned i = blockIdx.x * blockDim.x + threadIdx.x;\n  if (i < count) output[index[i]] = input[i];\n}\n",
    )
    .expect("Candidate task");
}

fn run_with_responses(
    request: ProposalHostRequestV1,
    responses: Vec<Vec<u8>>,
) -> cairn_migration::ProposalHostTerminalV1 {
    let state = tempfile::tempdir().expect("state");
    let mut content =
        SqliteContentStore::open(state.path().join("content.db"), state.path().join("cas"))
            .expect("content");
    let mut events = SqliteEventStore::open(state.path().join("events.db")).expect("events");
    let mut index = 0_usize;
    let mut transport = ScriptedModelTransport::new(
        move |_: &cairn_agent::PreparedModelRequest| -> Result<_, TransportError> {
            let response = responses.get(index).expect("recorded response").clone();
            index += 1;
            Ok(ModelTransportResponse::without_usage(response))
        },
    );
    match run_proposal_host_episode(&mut events, &mut content, &mut transport, codec(), request)
        .expect("Host episode")
    {
        cairn_migration::ProposalHostOutcomeV1::Terminal { terminal } => *terminal,
        cairn_migration::ProposalHostOutcomeV1::AwaitingController { .. } => {
            panic!("recorded local-tool profile unexpectedly requested a Controller experiment")
        }
    }
}

struct RecordedOracleEffectWorker {
    binding: ProposalHostWorkerBindingV1,
}

impl ProposalHostExperimentWorker for RecordedOracleEffectWorker {
    fn prepare(
        &mut self,
        _operation: &ProposalHostExperimentOperationV1,
    ) -> Result<ProposalHostWorkerBindingV1, ProposalHostExperimentWorkerError> {
        Ok(self.binding.clone())
    }

    fn execute(
        &mut self,
        dispatch: &ProposalHostExperimentDispatchV1,
    ) -> Result<ProposalHostWorkerObservationV1, ProposalHostExperimentWorkerError> {
        let bytes = cairn_codec::to_vec(&json!({
            "schema_version":1,
            "job_id":self.binding.job_id(),
            "attempt_id":self.binding.attempt_id(),
            "contract_id":self.binding.contract_id(),
            "outcome":"succeeded",
            "exit_code":0,
            "elapsed_ms":4,
            "stdout_id":id::<ExecutionStdoutArtifact>(b"research stdout"),
            "stderr_id":id::<ExecutionStderrArtifact>(b"research stderr"),
            "evidence_id":id::<ExecutionEvidenceArtifact>(b"research evidence"),
            "outputs":[]
        }))
        .expect("receipt bytes");
        let receipt: ExecutionReceipt = cairn_codec::from_slice(&bytes).expect("recorded receipt");
        let receipt_id =
            ContentId::<ExecutionReceiptArtifact>::derive(&bytes).expect("receipt identity");
        ProposalHostWorkerObservationV1::new(
            dispatch,
            receipt_id,
            receipt,
            json!({"matches":[],"query":"task-generic edge cases"}),
        )
        .map_err(|error| ProposalHostExperimentWorkerError::Rejected(error.to_string()))
    }
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the integration control keeps one exact Host journal across yield, effect, and resume"
)]
fn oracle_host_episode_is_bound_to_one_structured_claim_cell_and_preserves_unknown() {
    let task_root = tempfile::tempdir().expect("Oracle task");
    write_candidate_task(task_root.path());
    let task = SirTaskWorkspace::load(task_root.path(), SirTaskLimits::default())
        .expect("Oracle task workspace");
    let task_id = TaskId::new();
    let runtime = runtime(EpisodeId::new(), b"Oracle runtime");
    let contract = id::<MigrationIntentContractArtifact>(b"admitted intent");
    let specification = AuthoritativeIntentClaimV1::CollectionOutput(
        CollectionOutputIntentV1::exact_selected_occurrences(
            SirCallerClaimId::new("copies-strictly-above").expect("claim"),
            CollectionOutputOrderContractV1::UnspecifiedPermutation,
        ),
    );
    let claim = derive_oracle_claims(task_id, contract, &specification)
        .into_iter()
        .next()
        .expect("current V1 claim");
    let policy = OracleCoveragePolicyV1::new(
        OracleCoverageProfileV1::Correctness,
        OracleAdversarialPolicyV1::NotRequired,
    );
    let catalog = OracleStrategyCatalogV1::new(vec![
        OracleStrategyRegistrationV1::new(
            OracleStrategyName::new("model-synthesis").expect("strategy"),
            OracleStrategyKindV1::ModelBackedSynthesis,
            OracleStrategyExecutorV1::AgentEpisode {
                authorship_model: id::<ModelConfigurationArtifact>(b"Oracle authorship model"),
                invocation: runtime.identity().expect("Oracle invocation"),
                tools: OracleStrategyToolCatalogV1::standard()
                    .identity()
                    .expect("Oracle strategy tools"),
            },
            vec![OracleStrategyRoleV1::Synthesis],
            policy.concerns().to_vec(),
        )
        .expect("strategy registration"),
    ])
    .expect("strategy catalog");
    let workspace = OracleWorkspaceV1::new(&OracleWorkspaceInput {
        task_id,
        admitted_intent: contract,
        sir_input: id(b"SIR input"),
        sir_task_bundle: task.bundle().identity().expect("task bundle"),
        source: id::<OracleSourceSnapshotArtifact>(b"source snapshot"),
        documentation: ContentId::derive(b"Public API documentation.").expect("documentation"),
        build_and_tests: ContentId::derive(b"Build and test manifest.").expect("build/test"),
        knowledge: ContentId::derive(b"No additional knowledge.").expect("knowledge"),
        research_tools: id::<OracleResearchToolCatalogArtifact>(b"research tools"),
        experiment_tools: id::<OracleExperimentToolCatalogArtifact>(b"experiment tools"),
        capability_grant: id::<OracleExplorationCapabilityGrantArtifact>(b"capability grant"),
        coverage_policy: policy.identity().expect("coverage policy"),
        strategy_catalog: catalog.identity().expect("strategy catalog"),
        budget: OracleExplorationBudgetV1 {
            strategy_runs: OracleStrategyRunLimit::new(64).expect("strategy budget"),
            experiments: OracleExperimentLimit::new(16).expect("experiment budget"),
        },
    });
    let claim_id = claim.identity().expect("claim id");
    let item = derive_oracle_work_items(&[claim_id], &policy)
        .expect("work items")
        .into_iter()
        .next()
        .expect("one cell");
    let run = OracleStrategyRunV1::new(
        workspace.identity().expect("workspace"),
        &item,
        OracleStrategyName::new("model-synthesis").expect("strategy"),
        &catalog,
    )
    .expect("strategy run");
    let materials = ProposalHostOracleMaterialsV1::new(
        ProposalHostOracleDocumentationV1::new(
            workspace.documentation(),
            "Public API documentation.".into(),
        )
        .expect("documentation"),
        ProposalHostOracleBuildTestsV1::new(
            workspace.build_and_tests(),
            "Build and test manifest.".into(),
        )
        .expect("build/test"),
        ProposalHostOracleKnowledgeV1::new(
            workspace.knowledge(),
            "No additional knowledge.".into(),
        )
        .expect("knowledge"),
    );
    let request = ProposalHostRequestV1::new(
        runtime,
        ProposalHostRoleRequestV1::OracleStrategy {
            workspace,
            claim,
            work_item: item,
            run,
            task: ProposalHostTaskSnapshotV1::from_workspace(&task),
            materials,
        },
    )
    .expect("one-cell Oracle request");
    let mut drifted = serde_json::to_value(&request).expect("request value");
    drifted["role"]["work_item"]["concern"] = json!("allowed-result-relations");
    assert!(serde_json::from_value::<ProposalHostRequestV1>(drifted).is_err());

    let research = serde_json::to_string(&json!({
        "schema_version": 1,
        "query": "task-generic edge cases",
        "repositories": ["vendor/upstream"],
        "max_results": 2
    }))
    .expect("research arguments");
    let state = tempfile::tempdir().expect("Oracle Host state");
    let mut content =
        SqliteContentStore::open(state.path().join("content.db"), state.path().join("cas"))
            .expect("content");
    let mut events = SqliteEventStore::open(state.path().join("events.db")).expect("events");
    let research_response = serde_json::to_vec(&json!({"output":[{
        "type":"function_call", "call_id":"oracle-research",
        "name":"oracle_search_external_tests", "arguments":research
    }]}))
    .expect("research response");
    let mut first_transport = ScriptedModelTransport::new(
        move |_: &cairn_agent::PreparedModelRequest| -> Result<_, TransportError> {
            Ok(ModelTransportResponse::without_usage(
                research_response.clone(),
            ))
        },
    );
    let cairn_migration::ProposalHostOutcomeV1::AwaitingController { experiment } =
        run_proposal_host_episode(
            &mut events,
            &mut content,
            &mut first_transport,
            codec(),
            request.clone(),
        )
        .expect("Oracle Host yield")
    else {
        panic!("external Oracle research must yield to Controller authority");
    };
    assert_eq!(experiment.operations().len(), 1);
    assert_eq!(
        experiment.operations()[0].tool().as_str(),
        "oracle_search_external_tests"
    );
    assert_eq!(
        experiment.operations()[0].effect(),
        ToolEffectClass::Idempotent
    );

    let mut worker = RecordedOracleEffectWorker {
        binding: ProposalHostWorkerBindingV1::new(
            JobId::new(),
            AttemptId::new(),
            id::<JobContractArtifact>(b"research contract"),
        ),
    };
    let executed = execute_proposal_host_experiments(
        &mut events,
        &mut content,
        &request,
        &experiment,
        &mut worker,
    )
    .expect("Controller-authorized research");
    assert_eq!(executed.len(), 1);
    assert_eq!(
        execute_proposal_host_experiments(
            &mut events,
            &mut content,
            &request,
            &experiment,
            &mut worker,
        )
        .expect("exact effect replay"),
        executed
    );
    let oracle_observation = executed[0]
        .oracle_observation()
        .expect("Oracle-domain projection");
    let observation_id = oracle_observation.identity().expect("observation id");
    let ProposalHostRoleRequestV1::OracleStrategy { run, work_item, .. } = request.role() else {
        unreachable!()
    };
    assert_eq!(
        oracle_observation.item(),
        work_item.identity().expect("item")
    );
    assert_eq!(oracle_observation.run(), run.identity().expect("run"));

    let submit = serde_json::to_string(&json!({
        "outcome": "preserve-unknown",
        "schema_version": 1,
        "reason": "insufficient-observation",
        "observations": [observation_id]
    }))
    .expect("submission arguments");
    let responses = [
        serde_json::to_vec(&json!({"output":[{
            "type":"function_call", "call_id":"oracle-submit",
            "name":"oracle_submit_cell_result", "arguments":submit
        }]}))
        .expect("submit response"),
        serde_json::to_vec(&json!({"output":[{
            "type":"message", "id":"oracle-final", "phase":"final_answer",
            "role":"assistant", "status":"completed",
            "content":[{"type":"output_text","text":"submitted"}]
        }]}))
        .expect("final response"),
    ];
    let mut response_index = 0_usize;
    let mut resumed_transport = ScriptedModelTransport::new(
        move |_: &cairn_agent::PreparedModelRequest| -> Result<_, TransportError> {
            let response = responses[response_index].clone();
            response_index += 1;
            Ok(ModelTransportResponse::without_usage(response))
        },
    );
    let cairn_migration::ProposalHostOutcomeV1::Terminal { terminal } = run_proposal_host_episode(
        &mut events,
        &mut content,
        &mut resumed_transport,
        codec(),
        request,
    )
    .expect("same Oracle episode resume") else {
        panic!("resumed Oracle episode must terminate");
    };
    let ProposalHostPublicationV1::OracleStrategy { submission, .. } = terminal.publication()
    else {
        panic!("expected Oracle strategy publication");
    };
    let cairn_migration::OracleStrategySubmissionOutcomeV1::PreserveUnknown { evidence } =
        submission.result()
    else {
        panic!("expected explicit unknown");
    };
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].observations(), &[observation_id]);
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one integration control keeps exact admitted material, Host request, and typed publication visible"
)]
fn task_generic_candidate_host_consumes_exact_admitted_material_and_publishes_typed_source() {
    let task_root = tempfile::tempdir().expect("Candidate task");
    fs::write(
        task_root.path().join("scatter.cu"),
        "__global__ void scatter(const float* values, const unsigned* indices, float* output, unsigned count) {\n  unsigned i = blockIdx.x * blockDim.x + threadIdx.x;\n  if (i < count) output[indices[i]] = values[i];\n}\n",
    )
    .expect("materially different task source");
    let task = SirTaskWorkspace::load(task_root.path(), SirTaskLimits::default())
        .expect("Candidate task workspace");
    let task_id = TaskId::new();
    let admitted_intent = id::<MigrationIntentContractArtifact>(b"scatter admitted intent");
    let claim = OracleClaimV1::new(
        task_id,
        admitted_intent,
        OracleClaimName::new("scatter-observable-output").expect("claim name"),
        AuthoritativeIntentClaimV1::CollectionOutput(
            CollectionOutputIntentV1::exact_selected_occurrences(
                SirCallerClaimId::new("writes-indexed-values").expect("caller claim"),
                CollectionOutputOrderContractV1::StableInputRelative,
            ),
        ),
    );
    let claim_id = claim.identity().expect("claim id");
    let coverage = OracleCoveragePolicyV1::new(
        OracleCoverageProfileV1::Correctness,
        OracleAdversarialPolicyV1::NotRequired,
    );
    let item = derive_oracle_work_items(&[claim_id], &coverage)
        .expect("work items")
        .into_iter()
        .find(|item| {
            item.concern() == OracleConcernV1::ObservableOutputs
                && item.role() == OracleStrategyRoleV1::Synthesis
        })
        .expect("observable synthesis cell");
    let run = id::<OracleStrategyRunArtifact>(b"scatter reference run");
    let reference_bytes =
        b"Reference: each in-domain pair writes values[i] to output[indices[i]].".to_vec();
    let reference = ContentId::<cairn_verification::ReferenceArtifact>::derive(&reference_bytes)
        .expect("reference identity");
    let element = OraclePortfolioElementV1::new(
        item.identity().expect("item id"),
        run,
        OraclePortfolioElementKindV1::Reference(reference),
        vec![],
    )
    .expect("portfolio element");
    let proposal = id::<OraclePortfolioProposalArtifact>(b"scatter Oracle portfolio");
    let outcome = id::<OracleAdmissionOutcomeArtifact>(b"scatter Oracle outcome");
    let contract: CandidateOracleContractV1 = serde_json::from_value(json!({
        "schema_version": 1,
        "proposal": proposal,
        "outcome": outcome,
        "admitted_claims": [{
            "claim": claim_id,
            "entries": [{
                "item": item,
                "resolution": {
                    "status": "contributed",
                    "run": run,
                    "elements": [element.identity().expect("element id")],
                    "observations": []
                }
            }]
        }]
    }))
    .expect("Candidate contract");
    let material = CandidateOracleMaterialV1::from_portfolio_kind(element.kind(), reference_bytes)
        .expect("typed reference body");
    let oracle_materials = CandidateOracleMaterialsV1::new(
        &contract,
        vec![claim],
        vec![CandidateOracleElementMaterialV1::new(element, material).expect("element material")],
    )
    .expect("complete Oracle materials");
    let documentation_text = "Scatter API documentation.";
    let build_text = "Build the selected primary source as Ascend C.";
    let knowledge_text = "No task-specific recipe is preloaded.";
    let documentation =
        ContentId::<OracleDocumentationSnapshotArtifact>::derive(documentation_text.as_bytes())
            .expect("documentation id");
    let build_and_tests =
        ContentId::<OracleBuildTestSnapshotArtifact>::derive(build_text.as_bytes())
            .expect("build id");
    let knowledge = ContentId::<OracleKnowledgeSnapshotArtifact>::derive(knowledge_text.as_bytes())
        .expect("knowledge id");
    let workspace: CandidateWorkspaceV1 = serde_json::from_value(json!({
        "schema_version": 1,
        "task_id": task_id,
        "recovery_input": id::<cairn_migration::IntentRecoveryInputArtifact>(b"scatter recovery"),
        "admitted_intent": admitted_intent,
        "oracle_workspace": id::<OracleWorkspaceArtifact>(b"scatter Oracle workspace"),
        "oracle_contract": contract.identity().expect("contract id"),
        "task_bundle": task.bundle().identity().expect("task bundle"),
        "documentation": documentation,
        "build_and_tests": build_and_tests,
        "knowledge": knowledge
    }))
    .expect("Candidate workspace");
    let runtime = runtime(EpisodeId::new(), b"generic Candidate runtime");
    let request = ProposalHostRequestV1::new(
        runtime,
        ProposalHostRoleRequestV1::CandidateStrategy {
            workspace,
            contract,
            oracle_materials,
            task: ProposalHostTaskSnapshotV1::from_workspace(&task),
            public_materials: ProposalHostOracleMaterialsV1::new(
                ProposalHostOracleDocumentationV1::new(documentation, documentation_text.into())
                    .expect("documentation"),
                ProposalHostOracleBuildTestsV1::new(build_and_tests, build_text.into())
                    .expect("build/tests"),
                ProposalHostOracleKnowledgeV1::new(knowledge, knowledge_text.into())
                    .expect("knowledge"),
            ),
        },
    )
    .expect("task-generic Candidate request");
    let mut drifted = serde_json::to_value(&request).expect("request value");
    drifted["role"]["oracle_materials"]["elements"][0]["material"]["bytes"][0] = json!(0);
    assert!(serde_json::from_value::<ProposalHostRequestV1>(drifted).is_err());

    let submit = serde_json::to_string(&json!({
        "schema_version": 1,
        "files": [{
            "path": "src/scatter.asc",
            "source": "#include \"kernel_operator.h\"\nextern \"C\" __global__ __aicore__ void scatter_kernel() {}\n"
        }],
        "primary_source": "src/scatter.asc",
        "explanation": "Complete non-authoritative source for the frozen scatter authority."
    }))
    .expect("submit arguments");
    let terminal = run_with_responses(
        request.clone(),
        vec![
            serde_json::to_vec(&json!({"output":[{
                "type":"function_call", "call_id":"candidate-submit",
                "name":"candidate_submit_proposal", "arguments":submit
            }]}))
            .expect("submit response"),
            serde_json::to_vec(&json!({"output":[{
                "type":"message", "id":"candidate-final", "phase":"final_answer",
                "role":"assistant", "status":"completed",
                "content":[{"type":"output_text","text":"submitted"}]
            }]}))
            .expect("final response"),
        ],
    );
    terminal
        .validate_against(&request)
        .expect("Candidate terminal binding");
    let ProposalHostPublicationV1::CandidateStrategy { proposal, .. } = terminal.publication()
    else {
        panic!("expected generic Candidate publication");
    };
    assert_eq!(proposal.submission().files().len(), 1);
    assert_eq!(
        proposal.submission().primary_source().as_str(),
        "src/scatter.asc"
    );
}
