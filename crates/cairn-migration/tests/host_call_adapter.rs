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
    ArgumentIndex, AssembledBoundaryCaseInput, BooleanInputPattern, BufferAccessV1,
    BufferContractInput, BufferContractV1, BufferMemoryContractInput, BufferMemoryContractV1,
    BufferName, CallAdapterCaptureLimits, CallAdapterExecutableByteLimit,
    CallAdapterOutputBytesArtifact, CaseTarget, CorpusBufferByteLimit, CorpusElementCount,
    DataType, DimensionSpec, EntryPointName, ExtentValue, InclusiveExtentRange,
    InclusiveIntegerRange, InputValueCaseTarget, InputValueDisposition, InputValueDomainV1,
    IntegerValue, InvalidInputBehavior, MemoryConditionDisposition, MigrationDomainContractInput,
    MigrationDomainContractV1, MigrationExecutionNeed, MigrationValidationTier,
    PointerAlignmentContractV1, PreparedCallAdapterInput, PreparedCallAdapterJob,
    RequestedSemanticsArtifact, ScalarParameterContractInput, ScalarParameterContractV1,
    ScalarParameterName, ScalarParameterRole, SemanticClaimKind, ShapeSymbolContractInput,
    ShapeSymbolContractV1, ShapeSymbolName, ShapeSymbolSource, assemble_boundary_case_input,
    compose_call_adapter_job, derive_mandatory_base_cases, derive_mandatory_input_value_cases,
    materialize_input_value_case, prepare_boundary_call_adapter_input,
    validate_boundary_call_adapter_receipt,
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
        insufficient_capacity_non_empty: MemoryConditionDisposition::Invalid {
            behavior: invalid.clone(),
        },
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
