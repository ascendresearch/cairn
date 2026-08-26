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
    CallAdapterResultV1, CaseExpectedOutcome, CaseTarget, CorpusBufferByteLimit,
    CorpusElementCount, CorpusExecutionPlanArtifact, CorpusExecutionPlanError,
    CorpusExecutionPlanV1, CorpusExecutionReceipt, CorpusObservationSetArtifact,
    CorpusObservationSetError, CorpusObservationSetV1, DataType, DimensionSpec, EntryPointName,
    ExtentValue, InclusiveExtentRange, InclusiveIntegerRange, InputValueCaseTarget,
    InputValueDisposition, InputValueDomainV1, IntegerValue, InvalidInputBehavior,
    MandatoryInputValueCasesV1, MandatoryMemorySurfaceCasesV1, MemoryConditionDisposition,
    MigrationDomainContractInput, MigrationDomainContractV1, MigrationExecutionNeed,
    MigrationMandatoryCasesV1, MigrationValidationTier, PointerAlignmentContractV1,
    PreparedCallAdapterInput, PreparedCallAdapterJob, PreparedCorpusExecutionCase,
    PreparedCorpusExecutionPlan, RequestedSemanticsArtifact, ScalarParameterContractInput,
    ScalarParameterContractV1, ScalarParameterName, ScalarParameterRole, SemanticClaimKind,
    ShapeSymbolContractInput, ShapeSymbolContractV1, ShapeSymbolName, ShapeSymbolSource,
    ValidatedCorpusExecutionCase, ValidatedCorpusObservationSet, assemble_boundary_case_input,
    assemble_input_value_case_input, assemble_memory_surface_case_input, compose_call_adapter_job,
    derive_mandatory_base_cases, derive_mandatory_input_value_cases,
    derive_mandatory_memory_surface_cases, materialize_input_value_case,
    prepare_boundary_call_adapter_input, prepare_corpus_execution_plan,
    validate_boundary_call_adapter_receipt, validate_corpus_execution_receipts,
};
use cairn_protocol::{AttemptId, CommandId, ContentId, ContentType, JobId, ObservedAtUnixMillis};
use cairn_record::ContentStore;
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

const HOST_FIXTURE_BACKEND: &str = "host-fixture-v1";

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

struct CompletedCorpus {
    _directory: tempfile::TempDir,
    content: SqliteContentStore,
    plan: PreparedCorpusExecutionPlan,
    receipts: Vec<CorpusExecutionReceipt>,
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

fn assert_complete_plan_shape(
    prepared: &PreparedCorpusExecutionPlan,
    ordered: &PreparedCorpusExecutionPlan,
    quantitative: &MigrationMandatoryCasesV1,
    input_values: &MandatoryInputValueCasesV1,
    memory_surfaces: &MandatoryMemorySurfaceCasesV1,
) {
    assert_eq!(prepared.plan(), ordered.plan());
    assert_eq!(prepared.plan_id(), ordered.plan_id());
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

fn prepared_host_case() -> PreparedHostCase {
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

    let executable = fs::read(env!("CARGO_BIN_EXE_cairn-call-adapter-fixture"))
        .expect("fixture executable bytes");
    let adapter = prepare_boundary_call_adapter_input(
        &assembled,
        &executable,
        CallAdapterExecutableByteLimit::new(
            u64::try_from(executable.len()).expect("executable length"),
        )
        .expect("executable limit"),
    )
    .expect("adapter input");
    PreparedHostCase { assembled, adapter }
}

fn complete_host_case(prepared_case: PreparedHostCase) -> CompletedHostCase {
    let directory = tempfile::tempdir().expect("temporary execution state");
    let mut content = SqliteContentStore::open(
        directory.path().join("content.db"),
        directory.path().join("cas"),
    )
    .expect("content store");
    let mut events =
        SqliteEventStore::open(directory.path().join("events.db")).expect("event store");
    let archived_input =
        put::<InputBundleArtifact>(&mut content, prepared_case.adapter.input_bundle_bytes());
    assert_eq!(archived_input, prepared_case.adapter.input_bundle_id());
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
        &prepared_case.adapter,
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
    let bundle = prepared_case.adapter.input_bundle().clone();
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
    CompletedHostCase {
        _directory: directory,
        content,
        assembled: prepared_case.assembled,
        adapter: prepared_case.adapter,
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
        cases,
        b"ELF",
        CallAdapterExecutableByteLimit::new(16).expect("executable limit"),
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
    let plan = prepare_plan(&quantitative, &input_values, &memory_surfaces, cases)
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
        let capture = synthetic_corpus_capture(case, environment);
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

fn synthetic_corpus_capture(
    case: &PreparedCorpusExecutionCase,
    environment: ContentId<ExecutionEnvironmentArtifact>,
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
                    0_u8;
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
                    0_u8;
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
