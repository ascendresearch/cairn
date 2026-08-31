use std::{
    ffi::OsString, fs, io::Cursor, os::unix::fs::PermissionsExt as _, path::Path, process::Command,
};

use cairn_execution::{
    CapturedOutput, DiagnosticByteLimit, EvidenceByteLimit, ExecutionBackend, ExecutionCapture,
    ExecutionCompletion, ExecutionElapsedMillis, ExecutionEnvironmentArtifact, ExecutionInput,
    ExecutionJob, ExecutionJobState, ExecutionOutcome, ExecutionReceipt, ExecutionReceiptArtifact,
    ExecutorError, InputBundleArtifact, InputBundleEntry, InputBundleV1, InputFileMode,
    OutputByteLimit, ResolvedProgramIdentity, ScriptedExecutor, TrustedExecutionEvidence,
    authorize_execution_attempt, begin_execution_attempt, execute_execution_attempt,
    prepare_execution_job, recover_execution_job,
};
use cairn_migration::{
    ArgumentIndex, AssembledBoundaryCaseInput, AssembledCorpusExecutionCase, BooleanInputPattern,
    BufferAccessV1, BufferContractInput, BufferContractV1, BufferMemoryContractInput,
    BufferMemoryContractV1, BufferName, CallAdapterCaptureLimits, CallAdapterCompletionV1,
    CallAdapterExecutableByteLimit, CallAdapterObservedOutputV1, CallAdapterOutputBytesArtifact,
    CallAdapterResultV1, CaseExpectedOutcome, CaseTarget, CollectionF32Bits,
    CollectionOutputOracleDecisionV1, CollectionOutputOraclePolicyV1, CorpusBufferByteLimit,
    CorpusElementCount, CorpusExecutionPlanArtifact, CorpusExecutionPlanError,
    CorpusExecutionPlanV1, CorpusExecutionReceipt, CorpusExecutionSubjectV1,
    CorpusObservationSetArtifact, CorpusObservationSetError, CorpusObservationSetV1, DataType,
    DimensionSpec, EntryPointName, ExactCorpusComparisonArtifact, ExactCorpusComparisonError,
    ExactCorpusComparisonV1, ExactVariantTrialArtifact, ExactVariantTrialV1, ExtentValue,
    InclusiveExtentRange, InclusiveIntegerRange, InputValueCaseTarget, InputValueDisposition,
    InputValueDomainV1, IntegerValue, InvalidInputBehavior, MandatoryInputValueCasesV1,
    MandatoryMemorySurfaceCasesV1, MemoryConditionDisposition, MigrationDomainContractInput,
    MigrationDomainContractV1, MigrationExecutionNeed, MigrationIntentContractArtifact,
    MigrationMandatoryCasesV1, MigrationValidationTier, PointerAlignmentContractV1,
    PreparedCallAdapterInput, PreparedCallAdapterJob, PreparedCorpusExecutionCase,
    PreparedCorpusExecutionPlan, RequestedSemanticsArtifact, ScalarParameterContractInput,
    ScalarParameterContractV1, ScalarParameterName, ScalarParameterRole, SemanticClaimKind,
    ShapeSymbolContractInput, ShapeSymbolContractV1, ShapeSymbolName, ShapeSymbolSource,
    SirCallerClaimId, ValidatedCorpusExecutionCase, ValidatedCorpusObservationSet,
    ValidatedVariantBuild, VariantBuildCaptureLimits, VariantBuildDriverByteLimit,
    VariantBuildPlanArtifact, VariantBuildPlanV1, VariantBuildReceiptArtifact,
    VariantBuildReceiptV1, VariantExecutionError, VariantImplementationByteLimit,
    ZeroKMatmulF32OracleCaseV1, assemble_boundary_case_input, assemble_collection_f32_oracle_case,
    assemble_input_value_case_input, assemble_memory_surface_case_input,
    assemble_zero_k_matmul_f32_oracle, compare_exact_corpus_observations,
    compare_executable_oracle_output, compose_call_adapter_job, compose_exact_variant_trial,
    derive_mandatory_base_cases, derive_mandatory_input_value_cases,
    derive_mandatory_memory_surface_cases, materialize_collection_output_comparison,
    materialize_input_value_case, prepare_boundary_call_adapter_input,
    prepare_collection_output_call_adapter_input, prepare_corpus_execution_plan,
    prepare_executable_oracle_call_adapter_input, prepare_variant_build_job,
    validate_boundary_call_adapter_receipt, validate_collection_output_call_adapter_receipt,
    validate_corpus_execution_receipts, validate_executable_oracle_call_adapter_capture,
    validate_variant_build_receipt,
};
use cairn_protocol::{AttemptId, CommandId, ContentId, ContentType, JobId, ObservedAtUnixMillis};
use cairn_record::ContentStore;
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use cairn_verification::{
    ArtifactAuthorId, ArtifactAuthorshipV1, AuthorshipOrigin, ConstructionClaimArtifact,
    FaultClassName, FaultInjectionEvidenceArtifact, ImplementationBundleArtifact,
    ImplementationVariantArtifact, ImplementationVariantV1, ReferenceArtifact, VariantExpectation,
};

const HOST_FIXTURE_BACKEND: &str = "host-fixture-v1";

#[test]
fn model_authored_zero_k_oracle_runs_through_the_real_adapter_protocol() {
    let proposal: ZeroKMatmulF32OracleCaseV1 = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "case_name": "matmul-zero-k",
        "lhs_argument": 0,
        "rhs_argument": 1,
        "output_argument": 2,
        "lhs_shape": {"rows": 2, "columns": 0},
        "rhs_shape": {"rows": 0, "columns": 3},
        "output_shape": {"rows": 2, "columns": 3},
        "lhs_bits": [],
        "rhs_bits": [],
        "expected_output_bits": [0, 0, 0, 0, 0, 0],
        "comparison": "f32-numeric-exact"
    }))
    .expect("typed Oracle synthesis proposal");
    proposal
        .validate_matmul_zero_k_sample()
        .expect("sample contract");
    let assembled = assemble_zero_k_matmul_f32_oracle(&proposal).expect("materialized Oracle");
    let executable = fs::read(env!("CARGO_BIN_EXE_cairn-call-adapter-fixture"))
        .expect("fixture executable bytes");
    let prepared = prepare_executable_oracle_call_adapter_input(
        &assembled,
        &executable,
        CallAdapterExecutableByteLimit::new(
            u64::try_from(executable.len()).expect("executable length"),
        )
        .expect("executable limit"),
    )
    .expect("call-adapter input");

    let directory = tempfile::tempdir().expect("adapter roots");
    let input_root = directory.path().join("input");
    let output_root = directory.path().join("output");
    materialize_bundle(&input_root, prepared.input_bundle()).expect("materialize adapter input");
    fs::create_dir_all(&output_root).expect("output root");
    let process = Command::new(input_root.join("cairn/bin/call-adapter"))
        .args([
            "--request",
            input_root
                .join("cairn/call-adapter-request.json")
                .to_str()
                .expect("request path"),
            "--output-root",
            output_root.to_str().expect("output root path"),
        ])
        .env_clear()
        .output()
        .expect("run adapter");
    assert!(
        process.status.success(),
        "adapter stderr: {}",
        String::from_utf8_lossy(&process.stderr)
    );
    let observed_bytes =
        fs::read(output_root.join("cairn/abi/arg-00002.bin")).expect("observed output");
    let captured = vec![
        CapturedOutput {
            name: cairn_execution::OutputName::new("call-adapter-result").expect("result name"),
            bytes: fs::read(output_root.join("cairn/call-adapter-result.json"))
                .expect("result manifest"),
        },
        CapturedOutput {
            name: cairn_execution::OutputName::new("abi-output-00002").expect("output name"),
            bytes: observed_bytes.clone(),
        },
    ];
    let observation =
        validate_executable_oracle_call_adapter_capture(&assembled, &prepared, &captured)
            .expect("validated capture");
    assert_eq!(observation.result().outputs().len(), 1);
    assert!(
        compare_executable_oracle_output(&assembled, &observed_bytes)
            .expect("exact comparison")
            .matches()
    );
}

#[test]
fn admitted_policy_drives_receipt_bound_collection_materialization() {
    let decision = CollectionOutputOracleDecisionV1::new(
        ContentId::<MigrationIntentContractArtifact>::derive(b"generic admitted contract")
            .expect("contract identity"),
        SirCallerClaimId::new("copies-strictly-above").expect("selection claim"),
        CollectionOutputOraclePolicyV1::ExactMultisetAndCount,
    );
    let values = [1.0_f32, 4.0, 3.0, 2.0]
        .into_iter()
        .map(|value| CollectionF32Bits::new(value.to_bits()).expect("normal f32"))
        .collect::<Vec<_>>();
    let threshold = CollectionF32Bits::new(2.0_f32.to_bits()).expect("threshold");
    let assembled =
        assemble_collection_f32_oracle_case(&decision, &values, threshold).expect("case assembly");
    let executable = fs::read(env!(
        "CARGO_BIN_EXE_cairn-collection-output-adapter-fixture"
    ))
    .expect("collection fixture executable");
    let adapter = prepare_collection_output_call_adapter_input(
        &assembled,
        &executable,
        CallAdapterExecutableByteLimit::new(
            u64::try_from(executable.len()).expect("executable length"),
        )
        .expect("executable limit"),
    )
    .expect("adapter input");
    let completed = complete_adapter_input(adapter);
    let execution = validate_collection_output_call_adapter_receipt(
        &assembled,
        &completed.adapter,
        &completed.job,
        completed.receipt_id,
        &completed.receipt,
        &completed.content,
    )
    .expect("receipt-bound observation");
    let comparison = materialize_collection_output_comparison(
        &assembled,
        &decision,
        &execution,
        &completed.content,
    )
    .expect("receipt-bound comparison");

    assert!(
        comparison.matches(),
        "reordered actual child output is equivalent"
    );
    assert_eq!(
        comparison.id(),
        ContentId::derive(comparison.bytes()).expect("comparison evidence identity")
    );
    let invocation_json = serde_json::to_string(assembled.invocation()).expect("invocation JSON");
    assert!(!invocation_json.contains("expected"));
}

struct PreparedHostCase {
    assembled: AssembledBoundaryCaseInput,
    adapter: PreparedCallAdapterInput,
}

struct CompletedHostCase {
    _directory: tempfile::TempDir,
    content: SqliteContentStore,
    assembled: AssembledBoundaryCaseInput,
    adapter: PreparedCallAdapterInput,
    job: PreparedCallAdapterJob,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: ExecutionReceipt,
}

struct CompletedAdapterInput {
    directory: tempfile::TempDir,
    content: SqliteContentStore,
    adapter: PreparedCallAdapterInput,
    job: PreparedCallAdapterJob,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: ExecutionReceipt,
}

struct CompletedCorpus {
    _directory: tempfile::TempDir,
    content: SqliteContentStore,
    plan: PreparedCorpusExecutionPlan,
    receipts: Vec<CorpusExecutionReceipt>,
}

struct ExecutedVariantControls {
    domain: MigrationDomainContractV1,
    correct: ImplementationVariantV1,
    wrong: ImplementationVariantV1,
    correct_build: ValidatedVariantBuild,
    wrong_build: ValidatedVariantBuild,
    reference: CompletedCorpus,
    correct_run: CompletedCorpus,
    wrong_run: CompletedCorpus,
    reference_observations: ValidatedCorpusObservationSet,
    correct_observations: ValidatedCorpusObservationSet,
    wrong_observations: ValidatedCorpusObservationSet,
}

#[test]
fn host_fixture_runs_through_coordinator_receipt_and_typed_observation() {
    let prepared = prepared_host_case();
    assert_tampered_invocation_is_rejected(&prepared);
    let completed = complete_host_case(prepared);
    let validated = validate_boundary_call_adapter_receipt(
        &completed.assembled,
        &completed.adapter,
        &completed.job,
        completed.receipt_id,
        &completed.receipt,
        &completed.content,
    )
    .expect("validated adapter execution");
    assert_eq!(validated.receipt().outcome(), ExecutionOutcome::Succeeded);
    assert_eq!(validated.observation().result().outputs().len(), 1);
    assert_eq!(
        validated.observation().result().outputs()[0].bytes(),
        ContentId::<CallAdapterOutputBytesArtifact>::derive(&[0_u8; 2])
            .expect("zero output identity")
    );
}

#[test]
fn complete_corpus_plan_is_canonical_complete_and_strict_v1() {
    let domain = domain();
    let (quantitative, input_values, memory_surfaces, mut cases) = assembled_corpus(&domain);
    let original = cases.clone();
    let ordered = prepare_plan(
        &quantitative,
        &input_values,
        &memory_surfaces,
        original.clone(),
    )
    .expect("ordered complete corpus plan");
    let candidate = prepare_plan_for_subject(
        &quantitative,
        &input_values,
        &memory_surfaces,
        CorpusExecutionSubjectV1::Candidate {
            implementation: ContentId::<ImplementationBundleArtifact>::derive(
                b"source implementation",
            )
            .expect("implementation identity"),
        },
        original.clone(),
    )
    .expect("candidate corpus plan");
    cases.reverse();
    let prepared = prepare_plan(&quantitative, &input_values, &memory_surfaces, cases)
        .expect("complete corpus plan");

    assert_complete_plan_shape(
        &prepared,
        &ordered,
        &quantitative,
        &input_values,
        &memory_surfaces,
    );
    assert_ne!(prepared.plan(), candidate.plan());
    assert_ne!(prepared.plan_id(), candidate.plan_id());
    assert_incomplete_case_sets_rejected(&quantitative, &input_values, &memory_surfaces, &original);
    assert_persisted_plan_is_strict(prepared.plan());
}

#[test]
fn complete_corpus_receipts_are_exact_typed_and_not_a_verdict() {
    let completed = complete_synthetic_corpus();
    let ordered = validate_corpus_execution_receipts(
        &completed.plan,
        completed.receipts.clone(),
        &completed.content,
    )
    .expect("ordered corpus observations");
    let mut reversed = completed.receipts.clone();
    reversed.reverse();
    let collected =
        validate_corpus_execution_receipts(&completed.plan, reversed, &completed.content)
            .expect("complete corpus observations");

    assert_eq!(collected.observation_set(), ordered.observation_set());
    assert_eq!(collected.observation_set_id(), ordered.observation_set_id());
    assert_complete_observation_set(&completed, &collected);
    assert_receipt_set_failures(&completed);
    assert_persisted_observation_set_is_strict(collected.observation_set(), completed.plan.plan());
}

#[test]
fn exact_comparison_aligns_roles_obligations_and_value_identities() {
    let domain = domain();
    let reference = complete_synthetic_corpus_for(
        CorpusExecutionSubjectV1::Reference {
            reference: ContentId::<ReferenceArtifact>::derive(b"proposed exact reference")
                .expect("reference identity"),
        },
        0,
    );
    let matching_candidate = complete_synthetic_corpus_for(
        CorpusExecutionSubjectV1::Candidate {
            implementation: ContentId::<ImplementationBundleArtifact>::derive(b"candidate")
                .expect("candidate identity"),
        },
        0,
    );
    let different_candidate = complete_synthetic_corpus_for(
        CorpusExecutionSubjectV1::Candidate {
            implementation: ContentId::<ImplementationBundleArtifact>::derive(b"candidate")
                .expect("candidate identity"),
        },
        1,
    );
    let reference_observations = collect_completed_corpus(&reference);
    let matching_observations = collect_completed_corpus(&matching_candidate);
    let different_observations = collect_completed_corpus(&different_candidate);

    let matching = compare_exact_corpus_observations(
        &domain,
        &reference.plan,
        &reference_observations,
        &matching_candidate.plan,
        &matching_observations,
    )
    .expect("matching exact comparison");
    let different = compare_exact_corpus_observations(
        &domain,
        &reference.plan,
        &reference_observations,
        &different_candidate.plan,
        &different_observations,
    )
    .expect("different exact comparison");

    assert!(matching.comparison().all_match());
    assert!(!different.comparison().all_match());
    assert!(
        different
            .comparison()
            .comparisons()
            .iter()
            .any(|case| case.outputs().iter().any(|output| !output.matches()))
    );
    assert_eq!(
        matching.comparison_id(),
        ContentId::<ExactCorpusComparisonArtifact>::derive(matching.comparison_bytes())
            .expect("comparison identity")
    );
    matching
        .comparison()
        .validate_inputs(
            &domain,
            &reference.plan,
            &reference_observations,
            &matching_candidate.plan,
            &matching_observations,
        )
        .expect("recomputed exact comparison");
    assert_exact_comparison_is_strict(matching.comparison());
    assert_eq!(
        compare_exact_corpus_observations(
            &domain,
            &matching_candidate.plan,
            &matching_observations,
            &reference.plan,
            &reference_observations,
        ),
        Err(ExactCorpusComparisonError::ReferenceRoleRequired)
    );
    let mut numerical = serde_json::to_value(&domain).expect("domain JSON");
    numerical["semantic_claim"] = serde_json::json!("numerical");
    let numerical: MigrationDomainContractV1 =
        serde_json::from_value(numerical).expect("numerical domain");
    assert_eq!(
        compare_exact_corpus_observations(
            &numerical,
            &reference.plan,
            &reference_observations,
            &matching_candidate.plan,
            &matching_observations,
        ),
        Err(ExactCorpusComparisonError::NonExactDomain)
    );
}

#[test]
fn admission_variants_build_execute_observe_and_compare_through_shared_ports() {
    let fixture = executed_variant_controls();
    let correct_comparison = compare_exact_corpus_observations(
        &fixture.domain,
        &fixture.reference.plan,
        &fixture.reference_observations,
        &fixture.correct_run.plan,
        &fixture.correct_observations,
    )
    .expect("correct variant comparison");
    let wrong_comparison = compare_exact_corpus_observations(
        &fixture.domain,
        &fixture.reference.plan,
        &fixture.reference_observations,
        &fixture.wrong_run.plan,
        &fixture.wrong_observations,
    )
    .expect("wrong variant comparison");
    let correct_trial = compose_exact_variant_trial(
        &fixture.domain,
        &fixture.correct,
        &fixture.correct_build,
        &fixture.reference.plan,
        &fixture.reference_observations,
        &fixture.correct_run.plan,
        &fixture.correct_observations,
        &correct_comparison,
    )
    .expect("correct variant trial");
    let wrong_trial = compose_exact_variant_trial(
        &fixture.domain,
        &fixture.wrong,
        &fixture.wrong_build,
        &fixture.reference.plan,
        &fixture.reference_observations,
        &fixture.wrong_run.plan,
        &fixture.wrong_observations,
        &wrong_comparison,
    )
    .expect("wrong variant trial");

    assert!(correct_comparison.comparison().all_match());
    assert!(!wrong_comparison.comparison().all_match());
    assert!(
        correct_trial
            .trial()
            .expectation_satisfied(&correct_comparison)
    );
    assert!(wrong_trial.trial().expectation_satisfied(&wrong_comparison));
    assert_eq!(
        correct_trial.trial_id(),
        ContentId::<ExactVariantTrialArtifact>::derive(correct_trial.trial_bytes())
            .expect("variant trial identity")
    );
    correct_trial
        .trial()
        .validate_inputs(
            &fixture.domain,
            &fixture.correct,
            &fixture.correct_build,
            &fixture.reference.plan,
            &fixture.reference_observations,
            &fixture.correct_run.plan,
            &fixture.correct_observations,
            &correct_comparison,
        )
        .expect("recomputed correct trial");
    assert_exact_variant_trial_is_strict(correct_trial.trial());

    assert_eq!(
        compose_exact_variant_trial(
            &fixture.domain,
            &fixture.correct,
            &fixture.correct_build,
            &fixture.reference.plan,
            &fixture.reference_observations,
            &fixture.wrong_run.plan,
            &fixture.wrong_observations,
            &wrong_comparison,
        ),
        Err(VariantExecutionError::InconsistentVariantPlan)
    );
}

fn executed_variant_controls() -> ExecutedVariantControls {
    let domain = domain();
    let correct_executable = stripped_fixture(env!("CARGO_BIN_EXE_cairn-call-adapter-fixture"));
    let wrong_executable = stripped_fixture(env!("CARGO_BIN_EXE_cairn-call-adapter-wrong-fixture"));
    let correct = implementation_variant(
        &correct_executable,
        VariantExpectation::MustAccept {
            construction_claim: ContentId::<ConstructionClaimArtifact>::derive(
                b"fixture construction claim",
            )
            .expect("construction claim"),
        },
    );
    let wrong = implementation_variant(
        &wrong_executable,
        VariantExpectation::MustReject {
            fault_class: FaultClassName::new("zero-to-one-output").expect("fault class"),
            fault_evidence: ContentId::<FaultInjectionEvidenceArtifact>::derive(
                b"fixture one-output injection",
            )
            .expect("fault evidence"),
        },
    );
    assert_variant_implementation_mismatch(&correct, &wrong_executable);

    let correct_build = execute_variant_build(&correct, &correct_executable);
    let wrong_build = execute_variant_build(&wrong, &wrong_executable);
    assert_built_adapter_process(&correct_build, 0);
    assert_built_adapter_process(&wrong_build, 1);
    let reference = complete_synthetic_corpus_for_with_executable(
        CorpusExecutionSubjectV1::Reference {
            reference: ContentId::<ReferenceArtifact>::derive(b"exact fixture reference")
                .expect("reference"),
        },
        0,
        &correct_executable,
    );
    let correct_run = complete_synthetic_corpus_for_with_executable(
        CorpusExecutionSubjectV1::AdmissionVariant {
            variant: implementation_variant_id(&correct),
        },
        0,
        correct_build.executable_bytes(),
    );
    let wrong_run = complete_synthetic_corpus_for_with_executable(
        CorpusExecutionSubjectV1::AdmissionVariant {
            variant: implementation_variant_id(&wrong),
        },
        1,
        wrong_build.executable_bytes(),
    );
    let reference_observations = collect_completed_corpus(&reference);
    let correct_observations = collect_completed_corpus(&correct_run);
    let wrong_observations = collect_completed_corpus(&wrong_run);
    ExecutedVariantControls {
        domain,
        correct,
        wrong,
        correct_build,
        wrong_build,
        reference,
        correct_run,
        wrong_run,
        reference_observations,
        correct_observations,
        wrong_observations,
    }
}

fn assert_variant_implementation_mismatch(
    correct: &ImplementationVariantV1,
    wrong_executable: &[u8],
) {
    assert_eq!(
        prepare_variant_build_job(
            JobId::new(),
            correct,
            wrong_executable,
            byte_limit_for_implementation(wrong_executable),
            b"driver",
            VariantBuildDriverByteLimit::new(16).expect("driver limit"),
            ContentId::<ExecutionEnvironmentArtifact>::derive(b"build environment")
                .expect("environment"),
            &host_need(),
            variant_build_capture_limits(wrong_executable),
        ),
        Err(VariantExecutionError::ImplementationIdentityMismatch)
    );
}

fn assert_complete_plan_shape(
    prepared: &PreparedCorpusExecutionPlan,
    ordered: &PreparedCorpusExecutionPlan,
    quantitative: &MigrationMandatoryCasesV1,
    input_values: &MandatoryInputValueCasesV1,
    memory_surfaces: &MandatoryMemorySurfaceCasesV1,
) {
    assert_eq!(prepared.plan(), ordered.plan());
    assert_eq!(prepared.plan_id(), ordered.plan_id());
    assert!(matches!(
        prepared.plan().subject(),
        CorpusExecutionSubjectV1::Source { .. }
    ));
    assert_eq!(prepared.plan().items().len(), 8);
    assert_eq!(prepared.cases().len(), 8);
    assert_eq!(
        prepared
            .cases()
            .iter()
            .filter(|case| matches!(case, PreparedCorpusExecutionCase::Boundary { .. }))
            .count(),
        4
    );
    assert_eq!(
        prepared
            .cases()
            .iter()
            .filter(|case| matches!(case, PreparedCorpusExecutionCase::InputValue { .. }))
            .count(),
        3
    );
    assert_eq!(
        prepared
            .cases()
            .iter()
            .filter(|case| matches!(case, PreparedCorpusExecutionCase::MemorySurface { .. }))
            .count(),
        1
    );
    assert!(
        memory_surfaces
            .cases()
            .iter()
            .any(|case| case.disposition() == &MemoryConditionDisposition::Unknown),
        "unknown obligations stay in the committed set without becoming jobs"
    );
    prepared
        .plan()
        .validate_obligations(quantitative, input_values, memory_surfaces)
        .expect("obligation roots and executable subset");
    assert_eq!(
        prepared.plan_id(),
        ContentId::<CorpusExecutionPlanArtifact>::derive(prepared.plan_bytes())
            .expect("plan identity")
    );
    assert_eq!(
        cairn_codec::from_slice::<CorpusExecutionPlanV1>(prepared.plan_bytes())
            .expect("strict plan round trip"),
        *prepared.plan()
    );
}

fn assert_incomplete_case_sets_rejected(
    quantitative: &MigrationMandatoryCasesV1,
    input_values: &MandatoryInputValueCasesV1,
    memory_surfaces: &MandatoryMemorySurfaceCasesV1,
    original: &[AssembledCorpusExecutionCase],
) {
    let mut missing = original.to_vec();
    let removed = missing.pop().expect("one removable case");
    assert_eq!(
        prepare_plan(quantitative, input_values, memory_surfaces, missing),
        Err(CorpusExecutionPlanError::MissingCase {
            obligation: match removed {
                AssembledCorpusExecutionCase::Boundary { case, .. } => {
                    cairn_migration::CorpusObligationIdentityV1::Boundary {
                        case: case.manifest().boundary_case(),
                    }
                }
                AssembledCorpusExecutionCase::InputValue { case, .. } => {
                    cairn_migration::CorpusObligationIdentityV1::InputValue {
                        case: case.manifest().input_value_case(),
                    }
                }
                AssembledCorpusExecutionCase::MemorySurface { case, .. } => {
                    cairn_migration::CorpusObligationIdentityV1::MemorySurface {
                        case: case.manifest().memory_surface_case(),
                    }
                }
            },
        })
    );
    let mut duplicate = original.to_vec();
    duplicate.push(original[0].clone());
    assert!(matches!(
        prepare_plan(quantitative, input_values, memory_surfaces, duplicate),
        Err(CorpusExecutionPlanError::DuplicateCase { .. })
    ));
}

fn assert_persisted_plan_is_strict(plan: &CorpusExecutionPlanV1) {
    let value = serde_json::to_value(plan).expect("plan JSON");
    let mut wrong_version = value.clone();
    wrong_version["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<CorpusExecutionPlanV1>(wrong_version).is_err());
    let mut wrong_order = value.clone();
    wrong_order["items"]
        .as_array_mut()
        .expect("items")
        .reverse();
    assert!(serde_json::from_value::<CorpusExecutionPlanV1>(wrong_order).is_err());
    let mut duplicate_job = value.clone();
    duplicate_job["items"][1]["job_id"] = duplicate_job["items"][0]["job_id"].clone();
    assert!(serde_json::from_value::<CorpusExecutionPlanV1>(duplicate_job).is_err());
    let mut unknown_role = value.clone();
    unknown_role["subject"]["role"] = serde_json::json!("untrusted-reference");
    assert!(serde_json::from_value::<CorpusExecutionPlanV1>(unknown_role).is_err());
    let mut unknown = value;
    unknown["fallback_reader"] = serde_json::json!(true);
    assert!(serde_json::from_value::<CorpusExecutionPlanV1>(unknown).is_err());
}

fn assert_complete_observation_set(
    completed: &CompletedCorpus,
    collected: &ValidatedCorpusObservationSet,
) {
    assert_eq!(collected.cases().len(), completed.plan.cases().len());
    assert_eq!(collected.observation_set().plan(), completed.plan.plan_id());
    collected
        .observation_set()
        .validate_plan(completed.plan.plan())
        .expect("exact cited plan");
    assert_eq!(
        collected.observation_set_id(),
        ContentId::<CorpusObservationSetArtifact>::derive(collected.observation_set_bytes())
            .expect("observation-set identity")
    );
    assert_eq!(
        cairn_codec::from_slice::<CorpusObservationSetV1>(collected.observation_set_bytes())
            .expect("strict observation-set round trip"),
        *collected.observation_set()
    );
    assert_eq!(
        collected
            .cases()
            .iter()
            .filter(|case| matches!(case, ValidatedCorpusExecutionCase::Boundary { .. }))
            .count(),
        4
    );
    assert_eq!(
        collected
            .cases()
            .iter()
            .filter(|case| matches!(case, ValidatedCorpusExecutionCase::InputValue { .. }))
            .count(),
        3
    );
    assert_eq!(
        collected
            .cases()
            .iter()
            .filter(|case| matches!(case, ValidatedCorpusExecutionCase::MemorySurface { .. }))
            .count(),
        1
    );
}

fn assert_receipt_set_failures(completed: &CompletedCorpus) {
    let mut missing = completed.receipts.clone();
    let missing_job = missing.pop().expect("missing receipt").receipt().job_id();
    assert_eq!(
        validate_corpus_execution_receipts(&completed.plan, missing, &completed.content),
        Err(CorpusObservationSetError::MissingReceipt {
            job_id: missing_job
        })
    );

    let mut duplicate = completed.receipts.clone();
    duplicate.push(completed.receipts[0].clone());
    assert_eq!(
        validate_corpus_execution_receipts(&completed.plan, duplicate, &completed.content),
        Err(CorpusObservationSetError::DuplicateReceipt {
            job_id: completed.receipts[0].receipt().job_id()
        })
    );

    let mut swapped = completed.receipts.clone();
    swapped[0] = CorpusExecutionReceipt::new(swapped[1].receipt_id(), swapped[0].receipt().clone());
    assert!(matches!(
        validate_corpus_execution_receipts(&completed.plan, swapped, &completed.content),
        Err(CorpusObservationSetError::Receipt { .. })
    ));

    let unexpected = receipt_with_job(&completed.receipts[0], JobId::new());
    let unexpected_job = unexpected.receipt().job_id();
    let mut extra = completed.receipts.clone();
    extra.push(unexpected);
    assert_eq!(
        validate_corpus_execution_receipts(&completed.plan, extra, &completed.content),
        Err(CorpusObservationSetError::UnexpectedReceipt {
            job_id: unexpected_job
        })
    );
}

fn assert_persisted_observation_set_is_strict(
    set: &CorpusObservationSetV1,
    plan: &CorpusExecutionPlanV1,
) {
    let value = serde_json::to_value(set).expect("observation-set JSON");
    let mut wrong_version = value.clone();
    wrong_version["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<CorpusObservationSetV1>(wrong_version).is_err());
    let mut wrong_order = value.clone();
    wrong_order["observations"]
        .as_array_mut()
        .expect("observations")
        .reverse();
    assert!(serde_json::from_value::<CorpusObservationSetV1>(wrong_order).is_err());
    let mut duplicate_receipt = value.clone();
    duplicate_receipt["observations"][1]["receipt"] =
        duplicate_receipt["observations"][0]["receipt"].clone();
    assert!(serde_json::from_value::<CorpusObservationSetV1>(duplicate_receipt).is_err());
    let mut wrong_plan = value.clone();
    wrong_plan["plan"] = serde_json::to_value(
        ContentId::<CorpusExecutionPlanArtifact>::derive(b"another plan").expect("plan identity"),
    )
    .expect("plan identity JSON");
    let wrong_plan =
        serde_json::from_value::<CorpusObservationSetV1>(wrong_plan).expect("structural set");
    assert_eq!(
        wrong_plan.validate_plan(plan),
        Err(CorpusObservationSetError::InconsistentObservationSet)
    );
    let mut unknown = value;
    unknown["verdict"] = serde_json::json!("pass");
    assert!(serde_json::from_value::<CorpusObservationSetV1>(unknown).is_err());
}

fn assert_exact_comparison_is_strict(comparison: &ExactCorpusComparisonV1) {
    let value = serde_json::to_value(comparison).expect("comparison JSON");
    let mut wrong_version = value.clone();
    wrong_version["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<ExactCorpusComparisonV1>(wrong_version).is_err());
    let mut wrong_order = value.clone();
    wrong_order["comparisons"]
        .as_array_mut()
        .expect("comparisons")
        .reverse();
    assert!(serde_json::from_value::<ExactCorpusComparisonV1>(wrong_order).is_err());
    let mut duplicate_result = value.clone();
    duplicate_result["comparisons"][1]["reference_result"] =
        duplicate_result["comparisons"][0]["reference_result"].clone();
    assert!(serde_json::from_value::<ExactCorpusComparisonV1>(duplicate_result).is_err());
    let mut verdict = value;
    verdict["passed"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ExactCorpusComparisonV1>(verdict).is_err());
}

fn assert_exact_variant_trial_is_strict(trial: &ExactVariantTrialV1) {
    let value = serde_json::to_value(trial).expect("variant trial JSON");
    let mut wrong_version = value.clone();
    wrong_version["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<ExactVariantTrialV1>(wrong_version).is_err());
    let mut verdict = value;
    verdict["passed"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ExactVariantTrialV1>(verdict).is_err());
}

fn assert_variant_build_plan_is_strict(plan: &VariantBuildPlanV1) {
    let bytes = cairn_codec::to_vec(plan).expect("build plan bytes");
    assert_eq!(
        cairn_codec::from_slice::<VariantBuildPlanV1>(&bytes).expect("build plan round trip"),
        *plan
    );
    let value = serde_json::to_value(plan).expect("build plan JSON");
    let mut wrong_version = value.clone();
    wrong_version["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<VariantBuildPlanV1>(wrong_version).is_err());
    let mut unknown = value;
    unknown["fallback_builder"] = serde_json::json!(true);
    assert!(serde_json::from_value::<VariantBuildPlanV1>(unknown).is_err());
}

fn assert_variant_build_receipt_is_strict(receipt: &VariantBuildReceiptV1) {
    let bytes = cairn_codec::to_vec(receipt).expect("build receipt bytes");
    assert_eq!(
        cairn_codec::from_slice::<VariantBuildReceiptV1>(&bytes).expect("build receipt round trip"),
        *receipt
    );
    let value = serde_json::to_value(receipt).expect("build receipt JSON");
    let mut wrong_version = value.clone();
    wrong_version["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<VariantBuildReceiptV1>(wrong_version).is_err());
    let mut verdict = value;
    verdict["passed"] = serde_json::json!(true);
    assert!(serde_json::from_value::<VariantBuildReceiptV1>(verdict).is_err());
}

fn stripped_fixture(source: &str) -> Vec<u8> {
    let directory = tempfile::tempdir().expect("stripped fixture directory");
    let output = directory.path().join("fixture");
    let status = Command::new("strip")
        .args(["--strip-all", "-o"])
        .arg(&output)
        .arg(source)
        .status()
        .expect("GNU strip is required by the Linux host-fixture gate");
    assert!(status.success());
    fs::read(output).expect("stripped fixture bytes")
}

fn implementation_variant(
    executable: &[u8],
    expectation: VariantExpectation,
) -> ImplementationVariantV1 {
    ImplementationVariantV1::new(
        ContentId::<ImplementationBundleArtifact>::derive(executable)
            .expect("implementation identity"),
        expectation,
        ArtifactAuthorshipV1::new(
            AuthorshipOrigin::Repository,
            ArtifactAuthorId::new("host-variant-fixture").expect("author"),
            None,
            None,
        )
        .expect("authorship"),
    )
}

fn implementation_variant_id(
    variant: &ImplementationVariantV1,
) -> ContentId<ImplementationVariantArtifact> {
    ContentId::derive(&cairn_codec::to_vec(variant).expect("variant bytes"))
        .expect("variant identity")
}

fn host_need() -> MigrationExecutionNeed {
    MigrationExecutionNeed::new(
        MigrationValidationTier::V0Cpu,
        ExecutionBackend::new(HOST_FIXTURE_BACKEND).expect("backend"),
        cairn_execution::ExecutionTimeoutMillis::new(5_000).expect("timeout"),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
    )
    .expect("execution need")
}

fn byte_limit_for_implementation(bytes: &[u8]) -> VariantImplementationByteLimit {
    VariantImplementationByteLimit::new(
        u64::try_from(bytes.len()).expect("implementation byte length"),
    )
    .expect("implementation byte limit")
}

fn variant_build_capture_limits(bytes: &[u8]) -> VariantBuildCaptureLimits {
    VariantBuildCaptureLimits {
        stdout: OutputByteLimit::new(1_024).expect("stdout"),
        stderr: OutputByteLimit::new(1_024).expect("stderr"),
        executable: OutputByteLimit::new(
            u64::try_from(bytes.len()).expect("executable byte length"),
        )
        .expect("executable limit"),
        diagnostic: DiagnosticByteLimit::new(1_024).expect("diagnostic"),
        evidence: EvidenceByteLimit::new(4_096).expect("evidence"),
    }
}

fn execute_variant_build(
    variant: &ImplementationVariantV1,
    implementation: &[u8],
) -> ValidatedVariantBuild {
    let directory = tempfile::tempdir().expect("variant build state");
    let mut content = SqliteContentStore::open(
        directory.path().join("content.db"),
        directory.path().join("cas"),
    )
    .expect("content store");
    let mut events =
        SqliteEventStore::open(directory.path().join("events.db")).expect("event store");
    let environment = put::<ExecutionEnvironmentArtifact>(&mut content, b"build environment");
    let driver = stripped_fixture(env!("CARGO_BIN_EXE_cairn-variant-build-fixture"));
    let build = prepare_variant_build_job(
        JobId::new(),
        variant,
        implementation,
        byte_limit_for_implementation(implementation),
        &driver,
        VariantBuildDriverByteLimit::new(u64::try_from(driver.len()).expect("driver byte length"))
            .expect("driver limit"),
        environment,
        &host_need(),
        variant_build_capture_limits(implementation),
    )
    .expect("prepared variant build");
    assert_variant_build_plan_is_strict(build.plan());
    assert_eq!(
        put::<InputBundleArtifact>(&mut content, build.input_bundle_bytes()),
        build.input_bundle_id()
    );
    assert_eq!(
        put::<VariantBuildPlanArtifact>(&mut content, build.plan_bytes()),
        build.plan_id()
    );
    assert_variant_build_process(&build, implementation);
    let prepared = prepare_execution_job(&mut content, build.contract()).expect("prepared build");
    let authority = authorize_execution_attempt(
        &mut events,
        prepared,
        AttemptId::new(),
        &CommandId::new(),
        ObservedAtUnixMillis::new(1),
    )
    .expect("build authority");
    let started = begin_execution_attempt(
        &mut events,
        authority,
        &CommandId::new(),
        ObservedAtUnixMillis::new(2),
    )
    .expect("build start");
    let capture = synthetic_variant_build_capture(&build, environment, implementation);
    let mut executor =
        ScriptedExecutor::new(move |_input: &ExecutionInput<'_>| Ok(capture.clone()));
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
    .expect("build completion")
    else {
        panic!("expected completed build");
    };
    let validated = validate_variant_build_receipt(&build, receipt_id, &receipt, &content)
        .expect("validated variant build");
    validated
        .build_receipt()
        .validate_inputs(&build, receipt_id, &receipt, &content)
        .expect("recomputed build receipt");
    assert_variant_build_receipt_is_strict(validated.build_receipt());
    let mut changed: serde_json::Value =
        serde_json::to_value(&receipt).expect("generic build receipt JSON");
    changed["job_id"] = serde_json::to_value(JobId::new()).expect("job identity JSON");
    let changed: ExecutionReceipt =
        serde_json::from_value(changed).expect("changed generic build receipt");
    let changed_bytes = cairn_codec::to_vec(&changed).expect("changed receipt bytes");
    let changed_id = ContentId::<ExecutionReceiptArtifact>::derive(&changed_bytes)
        .expect("changed receipt identity");
    assert_eq!(
        validate_variant_build_receipt(&build, changed_id, &changed, &content),
        Err(VariantExecutionError::InconsistentBuildReceipt)
    );
    assert_eq!(validated.executable_bytes(), implementation);
    assert_eq!(
        put::<VariantBuildReceiptArtifact>(&mut content, validated.build_receipt_bytes()),
        validated.build_receipt_id()
    );
    validated
}

fn assert_variant_build_process(
    build: &cairn_migration::PreparedVariantBuildJob,
    implementation: &[u8],
) {
    let directory = tempfile::tempdir().expect("direct build fixture root");
    let input = directory.path().join("input");
    let output = directory.path().join("output");
    let work = directory.path().join("work");
    materialize_bundle(&input, build.input_bundle()).expect("materialized build fixture");
    fs::create_dir(&output).expect("build output root");
    fs::create_dir(&work).expect("build work root");
    let command = build.contract().command();
    let process = Command::new(input.join(command.program().as_str()))
        .args(
            command
                .arguments()
                .iter()
                .map(|argument| translate_argument(argument.as_str(), &input, &output)),
        )
        .current_dir(work)
        .env_clear()
        .output()
        .expect("build fixture process");
    assert!(process.status.success());
    assert_eq!(
        fs::read(output.join("cairn/call-adapter")).expect("built executable"),
        implementation
    );
}

fn synthetic_variant_build_capture(
    build: &cairn_migration::PreparedVariantBuildJob,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    implementation: &[u8],
) -> ExecutionCapture {
    let expected = &build.contract().capture().expected_outputs()[0];
    let evidence = TrustedExecutionEvidence::new(
        ExecutionBackend::new(HOST_FIXTURE_BACKEND).expect("backend"),
        environment,
        ResolvedProgramIdentity::new("variant-build-fixture-v1").expect("program identity"),
        Vec::new(),
    )
    .expect("build evidence");
    ExecutionCapture::new(
        ExecutionOutcome::Succeeded,
        Some(0),
        ExecutionElapsedMillis::new(1),
        Vec::new(),
        Vec::new(),
        vec![CapturedOutput {
            name: expected.name.clone(),
            bytes: implementation.to_vec(),
        }],
        evidence,
    )
}

fn prepared_host_case() -> PreparedHostCase {
    let executable = fs::read(env!("CARGO_BIN_EXE_cairn-call-adapter-fixture"))
        .expect("fixture executable bytes");
    prepared_host_case_with_executable(&executable)
}

fn prepared_host_case_with_executable(executable: &[u8]) -> PreparedHostCase {
    let domain = domain();
    let boundary = derive_mandatory_base_cases(&domain)
        .expect("boundary derivation")
        .cases()
        .iter()
        .find(|case| {
            matches!(
                case.target(),
                CaseTarget::ShapeSymbol { value, .. } if value.get() == 2
            )
        })
        .cloned()
        .expect("successful extent-two case");
    let input_case = derive_mandatory_input_value_cases(&domain)
        .expect("input derivation")
        .cases()
        .iter()
        .find(|case| {
            matches!(
                case.target(),
                InputValueCaseTarget::Boolean {
                    pattern: BooleanInputPattern::True,
                    ..
                }
            )
        })
        .cloned()
        .expect("true input case");
    assert_eq!(input_case.disposition(), &InputValueDisposition::Supported);
    let materialized = materialize_input_value_case(
        &input_case,
        CorpusElementCount::new(2),
        CorpusBufferByteLimit::new(64).expect("materialization limit"),
    )
    .expect("materialized input");
    let assembled = assemble_boundary_case_input(
        &domain,
        &boundary,
        &[materialized],
        CorpusBufferByteLimit::new(64).expect("assembly limit"),
    )
    .expect("assembled case");

    let adapter = prepare_boundary_call_adapter_input(
        &assembled,
        executable,
        CallAdapterExecutableByteLimit::new(
            u64::try_from(executable.len()).expect("executable length"),
        )
        .expect("executable limit"),
    )
    .expect("adapter input");
    PreparedHostCase { assembled, adapter }
}

fn complete_host_case(prepared_case: PreparedHostCase) -> CompletedHostCase {
    let completed = complete_adapter_input(prepared_case.adapter);
    CompletedHostCase {
        _directory: completed.directory,
        content: completed.content,
        assembled: prepared_case.assembled,
        adapter: completed.adapter,
        job: completed.job,
        receipt_id: completed.receipt_id,
        receipt: completed.receipt,
    }
}

fn complete_adapter_input(adapter: PreparedCallAdapterInput) -> CompletedAdapterInput {
    let directory = tempfile::tempdir().expect("temporary execution state");
    let mut content = SqliteContentStore::open(
        directory.path().join("content.db"),
        directory.path().join("cas"),
    )
    .expect("content store");
    let mut events =
        SqliteEventStore::open(directory.path().join("events.db")).expect("event store");
    let archived_input = put::<InputBundleArtifact>(&mut content, adapter.input_bundle_bytes());
    assert_eq!(archived_input, adapter.input_bundle_id());
    let environment =
        put::<ExecutionEnvironmentArtifact>(&mut content, b"host fixture environment");
    let need = MigrationExecutionNeed::new(
        MigrationValidationTier::V0Cpu,
        ExecutionBackend::new(HOST_FIXTURE_BACKEND).expect("backend"),
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
    assert_eq!(prepared.contract_id(), job.contract_id());
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

    let execution_root = directory.path().join("host-execution");
    let bundle = adapter.input_bundle().clone();
    let mut executor = ScriptedExecutor::new(|input: &ExecutionInput<'_>| {
        run_host_fixture(input, &bundle, &execution_root, environment)
    });
    assert!(matches!(
        execute_execution_attempt(
            &mut events,
            &mut content,
            &mut executor,
            started,
            &CommandId::new(),
            ObservedAtUnixMillis::new(3),
        )
        .expect("completed execution"),
        ExecutionCompletion::Completed { .. }
    ));

    drop(events);
    drop(content);
    let content = SqliteContentStore::open(
        directory.path().join("content.db"),
        directory.path().join("cas"),
    )
    .expect("reopened content store");
    let events =
        SqliteEventStore::open(directory.path().join("events.db")).expect("reopened event store");
    let execution_job = ExecutionJob::new(job.contract().job_id()).expect("execution job");
    let ExecutionJobState::Completed {
        receipt_id,
        receipt,
    } = recover_execution_job(&events, &content, &execution_job).expect("recovered execution")
    else {
        panic!("expected completed execution");
    };
    CompletedAdapterInput {
        directory,
        content,
        adapter,
        job,
        receipt_id,
        receipt,
    }
}

fn assert_tampered_invocation_is_rejected(prepared: &PreparedHostCase) {
    let directory = tempfile::tempdir().expect("tampered fixture root");
    let input = directory.path().join("input");
    let output = directory.path().join("output");
    let work = directory.path().join("work");
    materialize_bundle(&input, prepared.adapter.input_bundle()).expect("materialized fixture");
    fs::write(input.join("cairn/invocation.json"), b"tampered invocation")
        .expect("tampered invocation");
    fs::create_dir(&output).expect("output root");
    fs::create_dir(&work).expect("work root");
    let arguments = prepared
        .adapter
        .command()
        .arguments()
        .iter()
        .map(|argument| translate_argument(argument.as_str(), &input, &output));
    let process = Command::new(input.join(prepared.adapter.command().program().as_str()))
        .args(arguments)
        .current_dir(work)
        .env_clear()
        .output()
        .expect("tampered fixture process");
    assert!(!process.status.success());
    assert!(!output.join("cairn/call-adapter-result.json").exists());
}

fn assert_built_adapter_process(build: &ValidatedVariantBuild, output_byte: u8) {
    let prepared = prepared_host_case_with_executable(build.executable_bytes());
    let directory = tempfile::tempdir().expect("direct built adapter root");
    let input = directory.path().join("input");
    let output = directory.path().join("output");
    let work = directory.path().join("work");
    materialize_bundle(&input, prepared.adapter.input_bundle()).expect("materialized adapter");
    fs::create_dir(&output).expect("adapter output root");
    fs::create_dir(&work).expect("adapter work root");
    let command = prepared.adapter.command();
    let process = Command::new(input.join(command.program().as_str()))
        .args(
            command
                .arguments()
                .iter()
                .map(|argument| translate_argument(argument.as_str(), &input, &output)),
        )
        .current_dir(work)
        .env_clear()
        .output()
        .expect("built adapter process");
    assert!(process.status.success());
    for expected in prepared.adapter.request().expected_outputs() {
        let bytes = fs::read(output.join(expected.path().as_str())).expect("ABI output");
        assert_eq!(bytes, vec![output_byte; bytes.len()]);
    }
}

fn assembled_corpus(
    domain: &MigrationDomainContractV1,
) -> (
    MigrationMandatoryCasesV1,
    MandatoryInputValueCasesV1,
    MandatoryMemorySurfaceCasesV1,
    Vec<AssembledCorpusExecutionCase>,
) {
    let quantitative = derive_mandatory_base_cases(domain).expect("quantitative obligations");
    let input_values = derive_mandatory_input_value_cases(domain).expect("dtype obligations");
    let memory_surfaces =
        derive_mandatory_memory_surface_cases(domain).expect("memory obligations");
    let supported_input = input_values
        .cases()
        .iter()
        .find(|case| {
            matches!(
                case.target(),
                InputValueCaseTarget::Boolean {
                    pattern: BooleanInputPattern::True,
                    ..
                }
            )
        })
        .expect("supported baseline input");
    let baseline = quantitative
        .cases()
        .iter()
        .find(|case| {
            matches!(
                case.target(),
                CaseTarget::ShapeSymbol { value, .. } if value.get() == 2
            )
        })
        .expect("successful quantitative baseline");
    let limit = CorpusBufferByteLimit::new(64).expect("corpus limit");
    let mut cases = Vec::new();
    for case in quantitative.cases() {
        let count = case.shape_assignments()[0].value().get();
        let input =
            materialize_input_value_case(supported_input, CorpusElementCount::new(count), limit)
                .expect("quantitative input bytes");
        let case = assemble_boundary_case_input(domain, case, &[input], limit)
            .expect("quantitative assembly");
        cases.push(AssembledCorpusExecutionCase::Boundary {
            job_id: JobId::new(),
            case,
        });
    }
    for input_case in input_values.cases() {
        let input = materialize_input_value_case(input_case, CorpusElementCount::new(2), limit)
            .expect("dtype input bytes");
        let case = assemble_input_value_case_input(domain, baseline, input_case, &[input], limit)
            .expect("dtype assembly");
        cases.push(AssembledCorpusExecutionCase::InputValue {
            job_id: JobId::new(),
            case,
        });
    }
    let baseline_input =
        materialize_input_value_case(supported_input, CorpusElementCount::new(2), limit)
            .expect("memory baseline bytes");
    for memory_case in memory_surfaces
        .cases()
        .iter()
        .filter(|case| match case.disposition() {
            MemoryConditionDisposition::Supported => true,
            MemoryConditionDisposition::Invalid { behavior } => {
                behavior != &InvalidInputBehavior::ExplicitlyExcluded
            }
            MemoryConditionDisposition::ExplicitlyExcluded { .. }
            | MemoryConditionDisposition::Unknown => false,
        })
    {
        let case = assemble_memory_surface_case_input(
            domain,
            baseline,
            memory_case,
            std::slice::from_ref(&baseline_input),
            limit,
        )
        .expect("memory assembly");
        cases.push(AssembledCorpusExecutionCase::MemorySurface {
            job_id: JobId::new(),
            case,
        });
    }
    (quantitative, input_values, memory_surfaces, cases)
}

fn prepare_plan(
    quantitative: &MigrationMandatoryCasesV1,
    input_values: &MandatoryInputValueCasesV1,
    memory_surfaces: &MandatoryMemorySurfaceCasesV1,
    cases: Vec<AssembledCorpusExecutionCase>,
) -> Result<PreparedCorpusExecutionPlan, CorpusExecutionPlanError> {
    prepare_plan_for_subject(
        quantitative,
        input_values,
        memory_surfaces,
        CorpusExecutionSubjectV1::Source {
            implementation: ContentId::<ImplementationBundleArtifact>::derive(
                b"source implementation",
            )
            .expect("implementation identity"),
        },
        cases,
    )
}

fn prepare_plan_for_subject(
    quantitative: &MigrationMandatoryCasesV1,
    input_values: &MandatoryInputValueCasesV1,
    memory_surfaces: &MandatoryMemorySurfaceCasesV1,
    subject: CorpusExecutionSubjectV1,
    cases: Vec<AssembledCorpusExecutionCase>,
) -> Result<PreparedCorpusExecutionPlan, CorpusExecutionPlanError> {
    prepare_plan_for_subject_with_executable(
        quantitative,
        input_values,
        memory_surfaces,
        subject,
        cases,
        b"ELF",
    )
}

fn prepare_plan_for_subject_with_executable(
    quantitative: &MigrationMandatoryCasesV1,
    input_values: &MandatoryInputValueCasesV1,
    memory_surfaces: &MandatoryMemorySurfaceCasesV1,
    subject: CorpusExecutionSubjectV1,
    cases: Vec<AssembledCorpusExecutionCase>,
    executable: &[u8],
) -> Result<PreparedCorpusExecutionPlan, CorpusExecutionPlanError> {
    let need = MigrationExecutionNeed::new(
        MigrationValidationTier::V0Cpu,
        ExecutionBackend::new(HOST_FIXTURE_BACKEND).expect("backend"),
        cairn_execution::ExecutionTimeoutMillis::new(5_000).expect("timeout"),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
    )
    .expect("execution need");
    prepare_corpus_execution_plan(
        quantitative,
        input_values,
        memory_surfaces,
        subject,
        cases,
        executable,
        CallAdapterExecutableByteLimit::new(
            u64::try_from(executable.len()).expect("executable length"),
        )
        .expect("executable limit"),
        ContentId::<ExecutionEnvironmentArtifact>::derive(b"corpus environment")
            .expect("environment"),
        &need,
        CallAdapterCaptureLimits {
            stdout: OutputByteLimit::new(1_024).expect("stdout"),
            stderr: OutputByteLimit::new(1_024).expect("stderr"),
            result: OutputByteLimit::new(4_096).expect("result"),
            diagnostic: DiagnosticByteLimit::new(1_024).expect("diagnostic"),
            evidence: EvidenceByteLimit::new(4_096).expect("evidence"),
        },
    )
}

fn complete_synthetic_corpus() -> CompletedCorpus {
    complete_synthetic_corpus_for(
        CorpusExecutionSubjectV1::Source {
            implementation: ContentId::<ImplementationBundleArtifact>::derive(
                b"source implementation",
            )
            .expect("implementation identity"),
        },
        0,
    )
}

fn complete_synthetic_corpus_for(
    subject: CorpusExecutionSubjectV1,
    output_byte: u8,
) -> CompletedCorpus {
    complete_synthetic_corpus_for_with_executable(subject, output_byte, b"ELF")
}

fn complete_synthetic_corpus_for_with_executable(
    subject: CorpusExecutionSubjectV1,
    output_byte: u8,
    executable: &[u8],
) -> CompletedCorpus {
    let directory = tempfile::tempdir().expect("temporary corpus execution state");
    let mut content = SqliteContentStore::open(
        directory.path().join("content.db"),
        directory.path().join("cas"),
    )
    .expect("content store");
    let mut events =
        SqliteEventStore::open(directory.path().join("events.db")).expect("event store");
    let environment = put::<ExecutionEnvironmentArtifact>(&mut content, b"corpus environment");
    let domain = domain();
    let (quantitative, input_values, memory_surfaces, cases) = assembled_corpus(&domain);
    let plan = prepare_plan_for_subject_with_executable(
        &quantitative,
        &input_values,
        &memory_surfaces,
        subject,
        cases,
        executable,
    )
    .expect("prepared corpus plan");
    assert_eq!(
        put::<CorpusExecutionPlanArtifact>(&mut content, plan.plan_bytes()),
        plan.plan_id()
    );

    let mut receipts = Vec::with_capacity(plan.cases().len());
    for (index, case) in plan.cases().iter().enumerate() {
        assert_eq!(
            put::<InputBundleArtifact>(&mut content, case.input().input_bundle_bytes()),
            case.input().input_bundle_id()
        );
        let prepared = prepare_execution_job(&mut content, case.job().contract())
            .expect("prepared corpus job");
        let authority = authorize_execution_attempt(
            &mut events,
            prepared,
            AttemptId::new(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(i64::try_from(index * 3 + 1).expect("event time")),
        )
        .expect("corpus execution authority");
        let started = begin_execution_attempt(
            &mut events,
            authority,
            &CommandId::new(),
            ObservedAtUnixMillis::new(i64::try_from(index * 3 + 2).expect("event time")),
        )
        .expect("started corpus execution");
        let capture = synthetic_corpus_capture(case, environment, output_byte);
        let mut executor =
            ScriptedExecutor::new(move |_input: &ExecutionInput<'_>| Ok(capture.clone()));
        let completion = execute_execution_attempt(
            &mut events,
            &mut content,
            &mut executor,
            started,
            &CommandId::new(),
            ObservedAtUnixMillis::new(i64::try_from(index * 3 + 3).expect("event time")),
        )
        .expect("completed corpus execution");
        let ExecutionCompletion::Completed {
            receipt_id,
            receipt,
        } = completion
        else {
            panic!("expected authoritative corpus receipt");
        };
        receipts.push(CorpusExecutionReceipt::new(receipt_id, receipt));
    }
    CompletedCorpus {
        _directory: directory,
        content,
        plan,
        receipts,
    }
}

fn collect_completed_corpus(completed: &CompletedCorpus) -> ValidatedCorpusObservationSet {
    validate_corpus_execution_receipts(
        &completed.plan,
        completed.receipts.clone(),
        &completed.content,
    )
    .expect("collected corpus observations")
}

fn synthetic_corpus_capture(
    case: &PreparedCorpusExecutionCase,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    output_byte: u8,
) -> ExecutionCapture {
    let input = case.input();
    let completion = match case.descriptor().expected_outcome() {
        CaseExpectedOutcome::Success => CallAdapterCompletionV1::InvokedVoid,
        CaseExpectedOutcome::Invalid {
            behavior: InvalidInputBehavior::RejectBeforeExecution,
        } => CallAdapterCompletionV1::RejectedBeforeInvocation,
        CaseExpectedOutcome::Invalid {
            behavior: InvalidInputBehavior::ReturnStatus { status },
        } => CallAdapterCompletionV1::InvokedStatus { status: *status },
        CaseExpectedOutcome::Invalid {
            behavior: InvalidInputBehavior::ExplicitlyExcluded,
        } => panic!("excluded case entered executable corpus"),
    };
    let invoked = completion != CallAdapterCompletionV1::RejectedBeforeInvocation;
    let observed = if invoked {
        input
            .request()
            .expected_outputs()
            .iter()
            .map(|expected| {
                let bytes = vec![
                    output_byte;
                    usize::try_from(expected.byte_length().get())
                        .expect("synthetic output length")
                ];
                CallAdapterObservedOutputV1::from_bytes(
                    expected.argument_index(),
                    expected.buffer().clone(),
                    &bytes,
                )
                .expect("synthetic observed output")
            })
            .collect()
    } else {
        Vec::new()
    };
    let result = CallAdapterResultV1::new(
        input.request_id(),
        input.request().invocation(),
        completion,
        observed,
    )
    .expect("synthetic adapter result");
    let result_bytes = cairn_codec::to_vec(&result).expect("synthetic result bytes");
    let outputs = case
        .job()
        .contract()
        .capture()
        .expected_outputs()
        .iter()
        .map(|expected| CapturedOutput {
            name: expected.name.clone(),
            bytes: if expected.path == *input.request().result_path() {
                result_bytes.clone()
            } else {
                let output = input
                    .request()
                    .expected_outputs()
                    .iter()
                    .find(|output| output.path() == &expected.path)
                    .expect("declared ABI output");
                vec![
                    output_byte;
                    usize::try_from(output.byte_length().get()).expect("synthetic output length")
                ]
            },
        })
        .collect();
    let evidence = TrustedExecutionEvidence::new(
        case.job().contract().backend().clone(),
        environment,
        ResolvedProgramIdentity::new(input.request().executable().to_wire())
            .expect("synthetic executable identity"),
        Vec::new(),
    )
    .expect("synthetic execution evidence");
    ExecutionCapture::new(
        ExecutionOutcome::Succeeded,
        Some(0),
        ExecutionElapsedMillis::new(1),
        Vec::new(),
        Vec::new(),
        outputs,
        evidence,
    )
}

fn receipt_with_job(source: &CorpusExecutionReceipt, job_id: JobId) -> CorpusExecutionReceipt {
    let mut value = serde_json::to_value(source.receipt()).expect("receipt JSON");
    value["job_id"] = serde_json::to_value(job_id).expect("job identity JSON");
    let receipt: ExecutionReceipt = serde_json::from_value(value).expect("changed receipt");
    let bytes = cairn_codec::to_vec(&receipt).expect("changed receipt bytes");
    CorpusExecutionReceipt::new(
        ContentId::<ExecutionReceiptArtifact>::derive(&bytes).expect("changed receipt identity"),
        receipt,
    )
}

fn domain() -> MigrationDomainContractV1 {
    let buffer = BufferName::new("value").expect("buffer");
    let symbol = ShapeSymbolName::new("n").expect("symbol");
    let parameter = ScalarParameterName::new("n_arg").expect("parameter");
    let invalid = InvalidInputBehavior::RejectBeforeExecution;
    let memory = BufferMemoryContractV1::new(BufferMemoryContractInput {
        null_non_empty: MemoryConditionDisposition::Invalid {
            behavior: invalid.clone(),
        },
        alignment: PointerAlignmentContractV1::ByteAligned,
        insufficient_capacity_non_empty: MemoryConditionDisposition::Unknown,
    });
    MigrationDomainContractV1::new(MigrationDomainContractInput {
        source_entry_point: EntryPointName::new("zero_bool").expect("entry point"),
        buffers: vec![
            BufferContractV1::new(BufferContractInput {
                argument_index: ArgumentIndex::new(0),
                name: buffer,
                access: BufferAccessV1::InputOutput {
                    value_domain: InputValueDomainV1::Boolean,
                },
                data_type: DataType::Bool,
                shape: vec![DimensionSpec::Symbol {
                    symbol: symbol.clone(),
                }],
                memory,
            })
            .expect("buffer contract"),
        ],
        scalar_parameters: vec![
            ScalarParameterContractV1::new(ScalarParameterContractInput {
                argument_index: ArgumentIndex::new(1),
                name: parameter.clone(),
                role: ScalarParameterRole::ShapeExtent,
                data_type: DataType::I32,
                valid_range: InclusiveIntegerRange::new(IntegerValue::new(1), IntegerValue::new(2))
                    .expect("integer range"),
                invalid_behavior: invalid.clone(),
            })
            .expect("scalar contract"),
        ],
        shape_symbols: vec![
            ShapeSymbolContractV1::new(ShapeSymbolContractInput {
                name: symbol,
                valid_range: InclusiveExtentRange::new(ExtentValue::new(1), ExtentValue::new(2))
                    .expect("extent range"),
                source: ShapeSymbolSource::ScalarParameter { parameter },
                boundary_moduli: Vec::new(),
                invalid_behavior: invalid,
            })
            .expect("shape contract"),
        ],
        buffer_aliasing: Vec::new(),
        requested_semantics: ContentId::<RequestedSemanticsArtifact>::derive(b"zero bool")
            .expect("semantics identity"),
        semantic_claim: SemanticClaimKind::Exact,
        exclusions: Vec::new(),
    })
    .expect("migration domain")
}

fn run_host_fixture(
    input: &ExecutionInput<'_>,
    bundle: &InputBundleV1,
    root: &Path,
    environment: ContentId<ExecutionEnvironmentArtifact>,
) -> Result<ExecutionCapture, ExecutorError> {
    let input_root = root.join("input");
    let output_root = root.join("output");
    let work_root = root.join("work");
    materialize_bundle(&input_root, bundle)?;
    fs::create_dir_all(&output_root).map_err(not_started)?;
    fs::create_dir_all(&work_root).map_err(not_started)?;
    let contract = input.contract();
    let program = input_root.join(contract.command().program().as_str());
    let arguments = contract
        .command()
        .arguments()
        .iter()
        .map(|argument| translate_argument(argument.as_str(), &input_root, &output_root))
        .collect::<Vec<_>>();
    let process = Command::new(&program)
        .args(arguments)
        .current_dir(work_root)
        .env_clear()
        .output()
        .map_err(not_started)?;
    let outcome = if process.status.success() {
        ExecutionOutcome::Succeeded
    } else {
        ExecutionOutcome::SubjectFailed
    };
    let outputs = contract
        .capture()
        .expected_outputs()
        .iter()
        .map(|expected| {
            fs::read(output_root.join(expected.path.as_str()))
                .map(|bytes| CapturedOutput {
                    name: expected.name.clone(),
                    bytes,
                })
                .map_err(|error| ExecutorError::Ambiguous(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let program_bytes =
        fs::read(program).map_err(|error| ExecutorError::Ambiguous(error.to_string()))?;
    let program_id =
        ContentId::<cairn_migration::CallAdapterExecutableArtifact>::derive(&program_bytes)
            .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?;
    let evidence = TrustedExecutionEvidence::new(
        ExecutionBackend::new(HOST_FIXTURE_BACKEND)
            .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?,
        environment,
        ResolvedProgramIdentity::new(program_id.to_wire())
            .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?,
        Vec::new(),
    )
    .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?;
    Ok(ExecutionCapture::new(
        outcome,
        process.status.code(),
        ExecutionElapsedMillis::new(1),
        process.stdout,
        process.stderr,
        outputs,
        evidence,
    ))
}

fn materialize_bundle(root: &Path, bundle: &InputBundleV1) -> Result<(), ExecutorError> {
    fs::create_dir_all(root).map_err(not_started)?;
    for entry in bundle.entries() {
        let target = root.join(entry.path().as_str());
        match entry {
            InputBundleEntry::Directory { .. } => {
                fs::create_dir(&target).map_err(not_started)?;
            }
            InputBundleEntry::File { mode, bytes, .. } => {
                fs::write(&target, bytes).map_err(not_started)?;
                let permissions = match mode {
                    InputFileMode::Data => 0o600,
                    InputFileMode::Executable => 0o700,
                };
                fs::set_permissions(&target, fs::Permissions::from_mode(permissions))
                    .map_err(not_started)?;
            }
        }
    }
    Ok(())
}

fn translate_argument(value: &str, input: &Path, output: &Path) -> OsString {
    if value == "/cairn/output" {
        output.as_os_str().to_owned()
    } else if let Some(relative) = value.strip_prefix("/cairn/output/") {
        output.join(relative).into_os_string()
    } else if let Some(relative) = value.strip_prefix("/cairn/input/") {
        input.join(relative).into_os_string()
    } else {
        OsString::from(value)
    }
}

fn put<T: ContentType>(content: &mut SqliteContentStore, bytes: &[u8]) -> ContentId<T> {
    content
        .put::<T>(&mut Cursor::new(bytes))
        .expect("archive content")
        .content_id
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the helper is passed directly to Result::map_err, which supplies an owned I/O error"
)]
fn not_started(error: std::io::Error) -> ExecutorError {
    ExecutorError::NotStarted(error.to_string())
}
