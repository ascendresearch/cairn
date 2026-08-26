use std::{
    ffi::OsString, fs, io::Cursor, os::unix::fs::PermissionsExt as _, path::Path, process::Command,
};

use cairn_execution::{
    CapturedOutput, DiagnosticByteLimit, EvidenceByteLimit, ExecutionBackend, ExecutionCapture,
    ExecutionCompletion, ExecutionElapsedMillis, ExecutionEnvironmentArtifact, ExecutionInput,
    ExecutionOutcome, ExecutionReceiptArtifact, ExecutorError, InputBundleArtifact,
    InputBundleEntry, InputBundleV1, InputFileMode, OutputByteLimit, ResolvedProgramIdentity,
    ScriptedExecutor, TrustedExecutionEvidence, authorize_execution_attempt,
    begin_execution_attempt, execute_execution_attempt, prepare_execution_job,
};
use cairn_migration::{
    ArgumentIndex, BufferAccessV1, BufferContractInput, BufferContractV1,
    BufferMemoryContractInput, BufferMemoryContractV1, BufferName, DataType, DimensionSpec,
    EntryPointName, FiniteF32Bits, FloatingInputValueDomainInput, FloatingInputValueDomainV1,
    HistoricalDetectionRequirement, HistoricalFailureClassName, HistoricalFailureCoverageV1,
    HistoricalFailureEvidenceArtifact, HistoricalFailureObligationV1, HistoricalFailureRecordInput,
    HistoricalFailureRecordV1, HistoricalFailureScope, HistoricalObservedFailureArtifact,
    HistoricalReductionAlgorithm, HistoricalReductionCaptureLimits,
    HistoricalReductionCaseArtifact, HistoricalReductionCaseV1, HistoricalReductionControlArtifact,
    HistoricalReductionControlError, HistoricalReductionCorrectVariantEvidence,
    HistoricalReductionWrongVariantEvidence, HistoricalReproductionArtifact,
    HistoricalValidationStage, InputValueDomainV1, MemoryConditionDisposition,
    MigrationDomainContractInput, MigrationDomainContractV1, MigrationDomainFamilyName,
    MigrationExecutionNeed, MigrationValidationTier, OracleFailureMechanismName,
    PointerAlignmentContractV1, PreparedHistoricalReductionJob, RequestedSemanticsArtifact,
    SemanticClaimKind, ValidatedHistoricalReductionRun, ValidatedVariantBuild,
    VariantBuildCaptureLimits, VariantBuildDriverByteLimit, VariantImplementationByteLimit,
    compose_historical_reduction_control, prepare_historical_reduction_corpus,
    prepare_historical_reduction_reference_job, prepare_historical_reduction_variant_job,
    prepare_variant_build_job, validate_historical_reduction_receipt,
    validate_variant_build_receipt,
};
use cairn_protocol::{
    AttemptId, CommandId, ContentId, ContentType, JobId, ObservedAtUnixMillis, TaskId,
};
use cairn_record::ContentStore;
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use cairn_verification::{
    AdmissionCorpusArtifact, AdmissionExecutionScope, AdmissionPolicyInput, AdmissionPolicyV1,
    AllowanceAssurance, AllowanceMagnitude, AllowanceProvenance, ArtifactAuthorId,
    ArtifactAuthorshipV1, AuthorshipOrigin, BudgetExhaustionOutcome, CallerDomainEvidenceArtifact,
    ConstructionClaimArtifact, ConstructionClaimInput, ConstructionClaimV1, ConstructionClassName,
    ConstructionEvidenceArtifact, ConstructionJustification, ConstructionPrerequisiteArtifact,
    CorpusCaseArtifact, CorpusCaseEntryV1, CorpusCaseProvenanceArtifact, CorpusCaseSource,
    CorpusProposalArtifact, CorpusProposalInput, CorpusProposalV1, CorrectVariantMinimum,
    CoverageObligationArtifact, DeclaredDomainArtifact, DeclaredDomainV1, DomainRegionName,
    FaultClassName, FaultInjectionEvidenceArtifact, ImplementationBundleArtifact,
    ImplementationVariantArtifact, ImplementationVariantV1, IncorrectVariantMinimum,
    LicenseProvenanceArtifact, MutationCaseArtifact, MutationComparisonArtifact, MutationDetection,
    MutationExecutionArtifact, MutationGridCellV1, MutationInjectionArtifact, MutationSizing,
    MutationTrialV1, NonInjectableReasonArtifact, NumericalAllowanceInput, NumericalAllowanceV1,
    ObservationPlanArtifact, OracleProposalInput, OracleProposalV1, OracleStrength,
    OracleTaskInputArtifact, PreparedMutationGridProof, ReferenceArtifact, SaturationRoundCount,
    SourceAdmissionPlanArtifact, StructuralIndependenceRequirement, TransformationKindName,
    TrustedMutantDefinitionArtifact, TrustedMutantV1, ValidFamilyPlanArtifact, VariantExpectation,
    prepare_generic_mutant_set, prepare_mutation_grid, recompute_mutation_grid_proof,
};

const BACKEND: &str = "historical-reduction-host-v1";

struct CompletedRun {
    job: PreparedHistoricalReductionJob,
    run: ValidatedHistoricalReductionRun,
}

struct CorrectControl {
    variant: ImplementationVariantV1,
    claim: ConstructionClaimV1,
    completed: CompletedRun,
}

struct WrongControl {
    variant: ImplementationVariantV1,
    completed: CompletedRun,
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one integration control deliberately keeps the ordinary proposal, six real executions, calibration, mutation proof, and rejection checks together"
)]
fn historical_reduction_control_recomputes_false_reject_family_spread_and_blind_spot() {
    let domain = reduction_domain();
    let declared = declared_domain(&domain);
    let historical = historical_record();
    let obligation = HistoricalFailureObligationV1::from_record(
        &historical,
        HistoricalDetectionRequirement::OracleVerdictDivergence,
    )
    .expect("historical obligation");
    let corpus_proposal = corpus_proposal(&declared, &obligation);
    let reference_id = id::<ReferenceArtifact>(b"binary64-reduction-reference");
    let proposal = oracle_proposal(&declared, &corpus_proposal, reference_id);
    let corpus = reduction_corpus(&corpus_proposal);
    let reference = execute_reference(reference_id, &corpus);
    assert_strict_execution_artifacts(&reference);

    let sequential = execute_correct(
        "sequential-order",
        "left-fold",
        HistoricalReductionAlgorithm::Sequential,
        env!("CARGO_BIN_EXE_cairn-reduction-sequential-fixture"),
        &corpus,
    );
    let tree = execute_correct(
        "tree-order",
        "balanced-tree",
        HistoricalReductionAlgorithm::BalancedTree,
        env!("CARGO_BIN_EXE_cairn-reduction-tree-fixture"),
        &corpus,
    );
    let zero = execute_wrong(
        "zero-output",
        HistoricalReductionAlgorithm::ZeroOutput,
        env!("CARGO_BIN_EXE_cairn-reduction-zero-fixture"),
        &corpus,
    );
    let dropped = execute_wrong(
        "drop-last",
        HistoricalReductionAlgorithm::DropLast,
        env!("CARGO_BIN_EXE_cairn-reduction-drop-last-fixture"),
        &corpus,
    );
    let offset = execute_wrong(
        "unit-offset",
        HistoricalReductionAlgorithm::UnitOffset,
        env!("CARGO_BIN_EXE_cairn-reduction-offset-fixture"),
        &corpus,
    );

    let policy_mutation = mutation_control(&corpus, sequential.variant.implementation());
    let allowance = measured_allowance(&corpus);
    let coverage = HistoricalFailureCoverageV1::new(
        declared.body(),
        MigrationDomainFamilyName::new("reduction").expect("domain family"),
        vec![obligation.clone()],
    )
    .expect("historical coverage");
    let old_sample_case = corpus
        .corpus()
        .cases()
        .iter()
        .find(|case| case.body().inputs().len() == 4)
        .expect("historical sample")
        .case();
    let sequential_id = variant_id(&sequential.variant);
    let correct = [
        HistoricalReductionCorrectVariantEvidence {
            variant: &sequential.variant,
            construction_claim: &sequential.claim,
            job: &sequential.completed.job,
            run: &sequential.completed.run,
        },
        HistoricalReductionCorrectVariantEvidence {
            variant: &tree.variant,
            construction_claim: &tree.claim,
            job: &tree.completed.job,
            run: &tree.completed.run,
        },
    ];
    let wrong = [
        HistoricalReductionWrongVariantEvidence {
            variant: &zero.variant,
            job: &zero.completed.job,
            run: &zero.completed.run,
        },
        HistoricalReductionWrongVariantEvidence {
            variant: &dropped.variant,
            job: &dropped.completed.job,
            run: &dropped.completed.run,
        },
        HistoricalReductionWrongVariantEvidence {
            variant: &offset.variant,
            job: &offset.completed.job,
            run: &offset.completed.run,
        },
    ];
    let prepared = compose_historical_reduction_control(
        &domain,
        &declared,
        &corpus_proposal,
        &proposal,
        &historical,
        &obligation,
        &coverage,
        &policy_mutation.policy,
        &allowance,
        &corpus,
        old_sample_case,
        sequential_id,
        &reference.job,
        &reference.run,
        &correct,
        &wrong,
        &policy_mutation.mutants,
        &policy_mutation.grid,
        policy_mutation.proof.proof(),
    )
    .expect("complete historical control");

    assert_eq!(prepared.control().correct_trials().len(), 2);
    assert_eq!(prepared.control().wrong_trials().len(), 3);
    assert_eq!(prepared.control().old_single_sample_allowance().get(), 0);
    assert!(prepared.control().blind_spots().len() == 1);
    assert!(prepared.control().correct_trials().iter().any(|trial| {
        trial.algorithm() == HistoricalReductionAlgorithm::BalancedTree
            && trial.maximum_ulp_distance().get() == 1
    }));
    assert_eq!(
        ContentId::<HistoricalReductionControlArtifact>::derive(prepared.control_bytes())
            .expect("control identity"),
        prepared.control_id()
    );
    prepared
        .control()
        .validate_inputs(
            &domain,
            &declared,
            &corpus_proposal,
            &proposal,
            &historical,
            &obligation,
            &coverage,
            &policy_mutation.policy,
            &allowance,
            &corpus,
            old_sample_case,
            sequential_id,
            &reference.job,
            &reference.run,
            &correct,
            &wrong,
            &policy_mutation.mutants,
            &policy_mutation.grid,
            policy_mutation.proof.proof(),
        )
        .expect("recomputed control");

    assert_asserted_allowance_and_passed_tampering_fail(
        &prepared,
        &domain,
        &declared,
        &corpus_proposal,
        &proposal,
        &historical,
        &obligation,
        &coverage,
        &policy_mutation,
        &corpus,
        old_sample_case,
        sequential_id,
        &reference,
        &correct,
        &wrong,
    );
}

struct MutationControl {
    policy: AdmissionPolicyV1,
    mutants: cairn_verification::PreparedGenericMutantSet,
    grid: cairn_verification::PreparedMutationGrid,
    proof: PreparedMutationGridProof,
}

fn reduction_domain() -> MigrationDomainContractV1 {
    let supported = cairn_migration::InputValueDisposition::Supported;
    let floating = FloatingInputValueDomainV1::new(FloatingInputValueDomainInput {
        negative_zero: supported.clone(),
        subnormal: supported.clone(),
        infinity: supported.clone(),
        nan: supported,
    });
    let memory = BufferMemoryContractV1::new(BufferMemoryContractInput {
        null_non_empty: MemoryConditionDisposition::Invalid {
            behavior: cairn_migration::InvalidInputBehavior::RejectBeforeExecution,
        },
        alignment: PointerAlignmentContractV1::ByteAligned,
        insufficient_capacity_non_empty: MemoryConditionDisposition::Unknown,
    });
    MigrationDomainContractV1::new(MigrationDomainContractInput {
        source_entry_point: EntryPointName::new("reduce_f32").expect("entry point"),
        buffers: vec![
            BufferContractV1::new(BufferContractInput {
                argument_index: ArgumentIndex::new(0),
                name: BufferName::new("values_and_result").expect("buffer"),
                access: BufferAccessV1::InputOutput {
                    value_domain: InputValueDomainV1::Floating {
                        special_values: floating,
                    },
                },
                data_type: DataType::F32,
                shape: vec![DimensionSpec::Constant {
                    extent: cairn_migration::ExtentValue::new(4),
                }],
                memory,
            })
            .expect("buffer contract"),
        ],
        scalar_parameters: Vec::new(),
        shape_symbols: Vec::new(),
        buffer_aliasing: Vec::new(),
        requested_semantics: id::<RequestedSemanticsArtifact>(b"finite f32 reduction"),
        semantic_claim: SemanticClaimKind::Numerical,
        exclusions: Vec::new(),
    })
    .expect("reduction domain")
}

fn declared_domain(domain: &MigrationDomainContractV1) -> DeclaredDomainV1 {
    let bytes = cairn_codec::to_vec(domain).expect("domain bytes");
    DeclaredDomainV1::new(
        TaskId::new(),
        ContentId::derive(&bytes).expect("domain identity"),
        Vec::new(),
        id::<CallerDomainEvidenceArtifact>(b"historical caller declaration"),
    )
    .expect("declared domain")
}

fn historical_record() -> HistoricalFailureRecordV1 {
    HistoricalFailureRecordV1::new(HistoricalFailureRecordInput {
        failure_class: HistoricalFailureClassName::new("single-sample-false-reject")
            .expect("failure class"),
        domain_family: MigrationDomainFamilyName::new("reduction").expect("domain family"),
        scope: HistoricalFailureScope::Oracle {
            mechanism: OracleFailureMechanismName::new("single-sample-allowance")
                .expect("mechanism"),
        },
        observed_stage: HistoricalValidationStage::OracleComparison,
        source_evidence: vec![id::<HistoricalFailureEvidenceArtifact>(
            b"archived false reject",
        )],
        observed_failure: id::<HistoricalObservedFailureArtifact>(b"tree reduction rejected"),
        reproduction_fixture: id::<HistoricalReproductionArtifact>(b"offline reduction fixture"),
        license_provenance: id::<LicenseProvenanceArtifact>(b"project authored"),
    })
    .expect("historical record")
}

fn corpus_proposal(
    declared: &DeclaredDomainV1,
    obligation: &HistoricalFailureObligationV1,
) -> CorpusProposalV1 {
    let declared_bytes = cairn_codec::to_vec(declared).expect("declared domain bytes");
    let obligation_bytes = cairn_codec::to_vec(obligation).expect("obligation bytes");
    let mut cases = vec![
        CorpusCaseEntryV1::new(
            id::<CorpusCaseArtifact>(b"historical cancellation case"),
            CorpusCaseSource::HistoricalFailure,
            id::<CorpusCaseProvenanceArtifact>(b"historical fixture provenance"),
            id::<LicenseProvenanceArtifact>(b"project authored historical fixture"),
        ),
        CorpusCaseEntryV1::new(
            id::<CorpusCaseArtifact>(b"held out ordinary reduction"),
            CorpusCaseSource::TrustedBaseDerivation,
            id::<CorpusCaseProvenanceArtifact>(b"trusted held out derivation"),
            id::<LicenseProvenanceArtifact>(b"project authored held out fixture"),
        ),
    ];
    cases.sort_by_key(|case| case.case().to_wire());
    CorpusProposalV1::new(CorpusProposalInput {
        declared_domain: ContentId::<DeclaredDomainArtifact>::derive(&declared_bytes)
            .expect("declared identity"),
        refinements: Vec::new(),
        cases,
        coverage_obligations: vec![
            ContentId::<CoverageObligationArtifact>::derive(&obligation_bytes)
                .expect("coverage identity"),
        ],
    })
    .expect("corpus proposal")
}

fn oracle_proposal(
    declared: &DeclaredDomainV1,
    corpus: &CorpusProposalV1,
    reference: ContentId<ReferenceArtifact>,
) -> OracleProposalV1 {
    let declared_bytes = cairn_codec::to_vec(declared).expect("declared bytes");
    let corpus_bytes = cairn_codec::to_vec(corpus).expect("corpus bytes");
    OracleProposalV1::new(OracleProposalInput {
        task_id: declared.task_id(),
        task_inputs: id::<OracleTaskInputArtifact>(b"historical reduction task inputs"),
        declared_domain: ContentId::derive(&declared_bytes).expect("declared identity"),
        domain_refinements: Vec::new(),
        corpus_proposal: ContentId::derive(&corpus_bytes).expect("corpus identity"),
        references: vec![reference],
        properties: Vec::new(),
        source_admission_plan: id::<SourceAdmissionPlanArtifact>(b"source admission"),
        valid_family_plan: id::<ValidFamilyPlanArtifact>(b"two correct three wrong"),
        observation_plan: id::<ObservationPlanArtifact>(b"finite f32 result bits"),
        requested_strength: OracleStrength::Reference,
        authorship: authorship(),
    })
    .expect("oracle proposal")
}

fn reduction_corpus(
    proposal: &CorpusProposalV1,
) -> cairn_migration::PreparedHistoricalReductionCorpus {
    let proposal_bytes = cairn_codec::to_vec(proposal).expect("proposal corpus bytes");
    let historical = [0xc16d_ae47, 0x40c0_bab6, 0x38f1_e9f3, 0x43f0_3293]
        .into_iter()
        .map(|bits| FiniteF32Bits::new(bits).expect("finite historical input"))
        .collect();
    let held_out = [1.0_f32, 2.0, 3.0, 4.0]
        .into_iter()
        .map(|value| FiniteF32Bits::from_f32(value).expect("finite held out input"))
        .collect();
    prepare_historical_reduction_corpus(
        ContentId::<CorpusProposalArtifact>::derive(&proposal_bytes).expect("proposal identity"),
        vec![
            HistoricalReductionCaseV1::new(historical).expect("historical case"),
            HistoricalReductionCaseV1::new(held_out).expect("held out case"),
        ],
    )
    .expect("reduction corpus")
}

fn execute_reference(
    reference: ContentId<ReferenceArtifact>,
    corpus: &cairn_migration::PreparedHistoricalReductionCorpus,
) -> CompletedRun {
    let executable = stripped_fixture(env!("CARGO_BIN_EXE_cairn-reduction-reference-fixture"));
    let environment = environment_id();
    let job = prepare_historical_reduction_reference_job(
        JobId::new(),
        reference,
        HistoricalReductionAlgorithm::HighPrecisionReference,
        corpus,
        &executable,
        u64::try_from(executable.len()).expect("executable length"),
        environment,
        &execution_need(),
        reduction_limits(),
    )
    .expect("reference job");
    let run = execute_reduction_job(corpus, &job, environment);
    CompletedRun { job, run }
}

fn execute_correct(
    class: &str,
    transformation: &str,
    algorithm: HistoricalReductionAlgorithm,
    fixture: &str,
    corpus: &cairn_migration::PreparedHistoricalReductionCorpus,
) -> CorrectControl {
    let implementation = stripped_fixture(fixture);
    let claim = ConstructionClaimV1::new(ConstructionClaimInput {
        construction_class: ConstructionClassName::new(class).expect("construction class"),
        transformation: TransformationKindName::new(transformation).expect("transformation"),
        source_implementation: id::<ImplementationBundleArtifact>(b"source reduction"),
        prerequisites: vec![id::<ConstructionPrerequisiteArtifact>(
            b"finite reduction semantics",
        )],
        evidence: vec![id::<ConstructionEvidenceArtifact>(class.as_bytes())],
        justification: ConstructionJustification::StructuralArgument,
        authorship: authorship(),
    })
    .expect("construction claim");
    let claim_bytes = cairn_codec::to_vec(&claim).expect("claim bytes");
    let variant = ImplementationVariantV1::new(
        ContentId::<ImplementationBundleArtifact>::derive(&implementation)
            .expect("implementation identity"),
        VariantExpectation::MustAccept {
            construction_claim: ContentId::<ConstructionClaimArtifact>::derive(&claim_bytes)
                .expect("claim identity"),
        },
        authorship(),
    );
    let build = execute_variant_build(&variant, &implementation);
    let completed = execute_variant_run(&variant, algorithm, corpus, &build);
    CorrectControl {
        variant,
        claim,
        completed,
    }
}

fn execute_wrong(
    fault: &str,
    algorithm: HistoricalReductionAlgorithm,
    fixture: &str,
    corpus: &cairn_migration::PreparedHistoricalReductionCorpus,
) -> WrongControl {
    let implementation = stripped_fixture(fixture);
    let variant = ImplementationVariantV1::new(
        ContentId::<ImplementationBundleArtifact>::derive(&implementation)
            .expect("implementation identity"),
        VariantExpectation::MustReject {
            fault_class: FaultClassName::new(fault).expect("fault class"),
            fault_evidence: id::<FaultInjectionEvidenceArtifact>(fault.as_bytes()),
        },
        authorship(),
    );
    let build = execute_variant_build(&variant, &implementation);
    let completed = execute_variant_run(&variant, algorithm, corpus, &build);
    WrongControl { variant, completed }
}

fn execute_variant_build(
    variant: &ImplementationVariantV1,
    implementation: &[u8],
) -> ValidatedVariantBuild {
    let directory = tempfile::tempdir().expect("build state");
    let mut content = open_content(&directory);
    let mut events = open_events(&directory);
    let environment = put::<ExecutionEnvironmentArtifact>(&mut content, b"reduction environment");
    let driver = stripped_fixture(env!("CARGO_BIN_EXE_cairn-variant-build-fixture"));
    let build = prepare_variant_build_job(
        JobId::new(),
        variant,
        implementation,
        VariantImplementationByteLimit::new(
            u64::try_from(implementation.len()).expect("implementation length"),
        )
        .expect("implementation limit"),
        &driver,
        VariantBuildDriverByteLimit::new(u64::try_from(driver.len()).expect("driver length"))
            .expect("driver limit"),
        environment,
        &execution_need(),
        VariantBuildCaptureLimits {
            stdout: byte_limit(1_024),
            stderr: byte_limit(1_024),
            executable: byte_limit(
                u64::try_from(implementation.len()).expect("implementation length"),
            ),
            diagnostic: DiagnosticByteLimit::new(1_024).expect("diagnostic"),
            evidence: EvidenceByteLimit::new(4_096).expect("evidence"),
        },
    )
    .expect("prepared build");
    assert_eq!(
        put::<InputBundleArtifact>(&mut content, build.input_bundle_bytes()),
        build.input_bundle_id()
    );
    let (receipt_id, receipt) = execute_generic(
        &mut events,
        &mut content,
        build.contract(),
        build.input_bundle(),
        environment,
        directory.path().join("build-process"),
    );
    validate_variant_build_receipt(&build, receipt_id, &receipt, &content).expect("validated build")
}

fn execute_variant_run(
    variant: &ImplementationVariantV1,
    algorithm: HistoricalReductionAlgorithm,
    corpus: &cairn_migration::PreparedHistoricalReductionCorpus,
    build: &ValidatedVariantBuild,
) -> CompletedRun {
    let environment = environment_id();
    let job = prepare_historical_reduction_variant_job(
        JobId::new(),
        variant,
        algorithm,
        corpus,
        build,
        environment,
        &execution_need(),
        reduction_limits(),
    )
    .expect("variant reduction job");
    let run = execute_reduction_job(corpus, &job, environment);
    CompletedRun { job, run }
}

fn execute_reduction_job(
    corpus: &cairn_migration::PreparedHistoricalReductionCorpus,
    job: &PreparedHistoricalReductionJob,
    environment: ContentId<ExecutionEnvironmentArtifact>,
) -> ValidatedHistoricalReductionRun {
    let directory = tempfile::tempdir().expect("reduction execution state");
    let mut content = open_content(&directory);
    let mut events = open_events(&directory);
    assert_eq!(
        put::<ExecutionEnvironmentArtifact>(&mut content, b"reduction environment"),
        environment
    );
    assert_eq!(
        put::<InputBundleArtifact>(&mut content, job.input_bundle_bytes()),
        job.input_bundle_id()
    );
    let (receipt_id, receipt) = execute_generic(
        &mut events,
        &mut content,
        job.contract(),
        job.input_bundle(),
        environment,
        directory.path().join("reduction-process"),
    );
    let validated =
        validate_historical_reduction_receipt(corpus, job, receipt_id, &receipt, &content)
            .expect("validated reduction run");
    validated
        .execution_receipt()
        .validate_inputs(corpus, job, receipt_id, &receipt, &content)
        .expect("recomputed reduction receipt");
    validated
}

fn execute_generic(
    events: &mut SqliteEventStore,
    content: &mut SqliteContentStore,
    contract: &cairn_execution::JobContract,
    bundle: &InputBundleV1,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    root: std::path::PathBuf,
) -> (
    ContentId<ExecutionReceiptArtifact>,
    cairn_execution::ExecutionReceipt,
) {
    let prepared = prepare_execution_job(content, contract).expect("prepared generic execution");
    let authority = authorize_execution_attempt(
        events,
        prepared,
        AttemptId::new(),
        &CommandId::new(),
        ObservedAtUnixMillis::new(1),
    )
    .expect("execution authority");
    let started = begin_execution_attempt(
        events,
        authority,
        &CommandId::new(),
        ObservedAtUnixMillis::new(2),
    )
    .expect("started execution");
    let bundle = bundle.clone();
    let mut executor = ScriptedExecutor::new(move |input: &ExecutionInput<'_>| {
        run_process(input, &bundle, &root, environment)
    });
    let ExecutionCompletion::Completed {
        receipt_id,
        receipt,
    } = execute_execution_attempt(
        events,
        content,
        &mut executor,
        started,
        &CommandId::new(),
        ObservedAtUnixMillis::new(3),
    )
    .expect("completed execution")
    else {
        panic!("expected completed execution");
    };
    (receipt_id, receipt)
}

fn run_process(
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
    let process =
        Command::new(&program)
            .args(
                contract.command().arguments().iter().map(|argument| {
                    translate_argument(argument.as_str(), &input_root, &output_root)
                }),
            )
            .current_dir(work_root)
            .env_clear()
            .output()
            .map_err(not_started)?;
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
        ExecutionBackend::new(BACKEND)
            .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?,
        environment,
        ResolvedProgramIdentity::new(program_id.to_wire())
            .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?,
        Vec::new(),
    )
    .map_err(|error| ExecutorError::Ambiguous(error.to_string()))?;
    Ok(ExecutionCapture::new(
        if process.status.success() {
            ExecutionOutcome::Succeeded
        } else {
            ExecutionOutcome::SubjectFailed
        },
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
            InputBundleEntry::Directory { .. } => fs::create_dir(&target).map_err(not_started)?,
            InputBundleEntry::File { mode, bytes, .. } => {
                fs::write(&target, bytes).map_err(not_started)?;
                fs::set_permissions(
                    &target,
                    fs::Permissions::from_mode(match mode {
                        InputFileMode::Data => 0o600,
                        InputFileMode::Executable => 0o700,
                    }),
                )
                .map_err(not_started)?;
            }
        }
    }
    Ok(())
}

fn translate_argument(value: &str, input: &Path, output: &Path) -> OsString {
    if let Some(relative) = value.strip_prefix("/cairn/input/") {
        input.join(relative).into_os_string()
    } else if let Some(relative) = value.strip_prefix("/cairn/output/") {
        output.join(relative).into_os_string()
    } else {
        OsString::from(value)
    }
}

fn mutation_control(
    corpus: &cairn_migration::PreparedHistoricalReductionCorpus,
    subject: ContentId<ImplementationBundleArtifact>,
) -> MutationControl {
    let mut mutants = ["drop-last", "unit-offset", "zero-output"]
        .into_iter()
        .map(|fault| {
            TrustedMutantV1::new(
                id::<TrustedMutantDefinitionArtifact>(fault.as_bytes()),
                FaultClassName::new(fault).expect("fault class"),
            )
        })
        .collect::<Vec<_>>();
    mutants.sort_by_key(|mutant| mutant.definition().to_wire());
    let mutants = prepare_generic_mutant_set(mutants).expect("mutant set");
    let policy = AdmissionPolicyV1::new(AdmissionPolicyInput {
        mutant_set: mutants.mutant_set_id(),
        minimum_correct_variants: CorrectVariantMinimum::new(2).expect("correct minimum"),
        minimum_incorrect_variants: IncorrectVariantMinimum::new(3).expect("wrong minimum"),
        required_construction_classes: vec![
            ConstructionClassName::new("sequential-order").expect("class"),
            ConstructionClassName::new("tree-order").expect("class"),
        ],
        required_fault_classes: vec![
            FaultClassName::new("drop-last").expect("fault"),
            FaultClassName::new("unit-offset").expect("fault"),
            FaultClassName::new("zero-output").expect("fault"),
        ],
        structural_independence: StructuralIndependenceRequirement::DistinctConstructionClaims,
        saturation_rounds: SaturationRoundCount::new(2).expect("rounds"),
        accepted_strengths: vec![OracleStrength::Reference],
        required_execution_scopes: vec![
            AdmissionExecutionScope::ObservationPipeline,
            AdmissionExecutionScope::Implementation,
        ],
        budget_exhaustion_outcome: BudgetExhaustionOutcome::Unverifiable,
    })
    .expect("admission policy");
    let case = id::<MutationCaseArtifact>(b"accumulation-order case");
    let trials = mutants
        .mutant_set()
        .mutants()
        .iter()
        .enumerate()
        .map(|(index, mutant)| {
            let cell = MutationGridCellV1::new(mutant.definition(), case);
            MutationTrialV1::applied(
                cell,
                if index == 0 {
                    MutationSizing::CaseDependent
                } else {
                    MutationSizing::ScaleFree
                },
                id::<MutationInjectionArtifact>(mutant.definition().to_wire().as_bytes()),
                id::<MutationExecutionArtifact>(b"real implementation observation path"),
                vec![
                    AdmissionExecutionScope::ObservationPipeline,
                    AdmissionExecutionScope::Implementation,
                ],
                id::<MutationComparisonArtifact>(mutant.fault_class().as_str().as_bytes()),
                if index == 0 {
                    MutationDetection::Missed
                } else {
                    MutationDetection::Detected
                },
            )
        })
        .collect();
    let admission_corpus = ContentId::<AdmissionCorpusArtifact>::derive(corpus.corpus_bytes())
        .expect("admission corpus");
    let grid = prepare_mutation_grid(
        &policy,
        &mutants,
        subject,
        admission_corpus,
        vec![case],
        trials,
    )
    .expect("mutation grid");
    let proof = recompute_mutation_grid_proof(&policy, &mutants, &grid).expect("mutation proof");
    assert!(proof.proof().obligations_satisfied());
    assert_eq!(proof.proof().blind_spots().len(), 1);
    MutationControl {
        policy,
        mutants,
        grid,
        proof,
    }
}

fn measured_allowance(
    corpus: &cairn_migration::PreparedHistoricalReductionCorpus,
) -> NumericalAllowanceV1 {
    NumericalAllowanceV1::new(NumericalAllowanceInput {
        absolute: Some(AllowanceMagnitude::new("1").expect("one ULP")),
        relative: None,
        provenance: AllowanceProvenance::MeasuredFamily,
        assurance: AllowanceAssurance::HeldOutValidated,
        derivation_corpora: vec![
            ContentId::<AdmissionCorpusArtifact>::derive(corpus.corpus_bytes())
                .expect("derivation corpus"),
        ],
        validation_corpora: vec![id::<AdmissionCorpusArtifact>(
            b"held out reduction validation",
        )],
        domain_regions: vec![DomainRegionName::new("finite-f32-reductions").expect("region")],
    })
    .expect("measured allowance")
}

#[expect(
    clippy::too_many_arguments,
    reason = "the rejection control intentionally reuses every independently validated admission input"
)]
fn assert_asserted_allowance_and_passed_tampering_fail(
    prepared: &cairn_migration::PreparedHistoricalReductionControl,
    domain: &MigrationDomainContractV1,
    declared: &DeclaredDomainV1,
    corpus_proposal: &CorpusProposalV1,
    proposal: &OracleProposalV1,
    historical: &HistoricalFailureRecordV1,
    obligation: &HistoricalFailureObligationV1,
    coverage: &HistoricalFailureCoverageV1,
    mutation: &MutationControl,
    corpus: &cairn_migration::PreparedHistoricalReductionCorpus,
    old_sample_case: ContentId<HistoricalReductionCaseArtifact>,
    old_baseline_variant: ContentId<ImplementationVariantArtifact>,
    reference: &CompletedRun,
    correct: &[HistoricalReductionCorrectVariantEvidence<'_>],
    wrong: &[HistoricalReductionWrongVariantEvidence<'_>],
) {
    let asserted = NumericalAllowanceV1::new(NumericalAllowanceInput {
        absolute: Some(AllowanceMagnitude::new("1").expect("asserted magnitude")),
        relative: None,
        provenance: AllowanceProvenance::Asserted,
        assurance: AllowanceAssurance::ProvenBound,
        derivation_corpora: Vec::new(),
        validation_corpora: Vec::new(),
        domain_regions: vec![DomainRegionName::new("finite-f32-reductions").expect("region")],
    })
    .expect("record asserted allowance without trusting it");
    assert_eq!(
        compose_historical_reduction_control(
            domain,
            declared,
            corpus_proposal,
            proposal,
            historical,
            obligation,
            coverage,
            &mutation.policy,
            &asserted,
            corpus,
            old_sample_case,
            old_baseline_variant,
            &reference.job,
            &reference.run,
            correct,
            wrong,
            &mutation.mutants,
            &mutation.grid,
            mutation.proof.proof(),
        ),
        Err(HistoricalReductionControlError::InadmissibleAllowance)
    );

    let mutation_case = mutation.grid.grid().cases()[0];
    let non_injectable = mutation
        .mutants
        .mutant_set()
        .mutants()
        .iter()
        .map(|mutant| {
            MutationTrialV1::not_injectable(
                MutationGridCellV1::new(mutant.definition(), mutation_case),
                id::<NonInjectableReasonArtifact>(mutant.definition().to_wire().as_bytes()),
            )
        })
        .collect();
    let empty_grid = prepare_mutation_grid(
        &mutation.policy,
        &mutation.mutants,
        mutation.grid.grid().subject(),
        mutation.grid.grid().corpus(),
        vec![mutation_case],
        non_injectable,
    )
    .expect("complete but empty-applicable grid");
    let empty_proof =
        recompute_mutation_grid_proof(&mutation.policy, &mutation.mutants, &empty_grid)
            .expect("empty-applicable proof");
    assert!(!empty_proof.proof().obligations_satisfied());
    assert_eq!(
        compose_historical_reduction_control(
            domain,
            declared,
            corpus_proposal,
            proposal,
            historical,
            obligation,
            coverage,
            &mutation.policy,
            &measured_allowance(corpus),
            corpus,
            old_sample_case,
            old_baseline_variant,
            &reference.job,
            &reference.run,
            correct,
            wrong,
            &mutation.mutants,
            &empty_grid,
            empty_proof.proof(),
        ),
        Err(HistoricalReductionControlError::InsufficientMutationControl)
    );

    let mut value: serde_json::Value =
        serde_json::from_slice(prepared.control_bytes()).expect("control JSON");
    value["passed"] = serde_json::json!(true);
    assert!(
        serde_json::from_value::<cairn_migration::HistoricalReductionControlV1>(value).is_err()
    );
    let mut value: serde_json::Value =
        serde_json::from_slice(prepared.control_bytes()).expect("control JSON");
    value["schema_version"] = serde_json::json!(2);
    assert!(
        serde_json::from_value::<cairn_migration::HistoricalReductionControlV1>(value).is_err()
    );
}

fn authorship() -> ArtifactAuthorshipV1 {
    ArtifactAuthorshipV1::new(
        AuthorshipOrigin::Repository,
        ArtifactAuthorId::new("cairn-historical-control").expect("author"),
        None,
        None,
    )
    .expect("authorship")
}

fn assert_strict_execution_artifacts(reference: &CompletedRun) {
    let mut plan = serde_json::to_value(reference.job.plan()).expect("plan JSON");
    plan["schema_version"] = serde_json::json!(2);
    assert!(
        serde_json::from_value::<cairn_migration::HistoricalReductionExecutionPlanV1>(plan)
            .is_err()
    );
    let mut receipt =
        serde_json::to_value(reference.run.execution_receipt()).expect("receipt JSON");
    receipt["unknown"] = serde_json::json!(true);
    assert!(
        serde_json::from_value::<cairn_migration::HistoricalReductionExecutionReceiptV1>(receipt)
            .is_err()
    );
    let mut observation =
        serde_json::to_value(reference.run.observation()).expect("observation JSON");
    observation["schema_version"] = serde_json::json!(2);
    assert!(
        serde_json::from_value::<cairn_migration::HistoricalReductionFixtureOutputV1>(observation)
            .is_err()
    );
}

fn variant_id(variant: &ImplementationVariantV1) -> ContentId<ImplementationVariantArtifact> {
    ContentId::derive(&cairn_codec::to_vec(variant).expect("variant bytes"))
        .expect("variant identity")
}

fn execution_need() -> MigrationExecutionNeed {
    MigrationExecutionNeed::new(
        MigrationValidationTier::V0Cpu,
        ExecutionBackend::new(BACKEND).expect("backend"),
        cairn_execution::ExecutionTimeoutMillis::new(5_000).expect("timeout"),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
    )
    .expect("execution need")
}

fn reduction_limits() -> HistoricalReductionCaptureLimits {
    HistoricalReductionCaptureLimits {
        stdout: byte_limit(1_024),
        stderr: byte_limit(1_024),
        observation: byte_limit(64 * 1_024),
        diagnostic: DiagnosticByteLimit::new(1_024).expect("diagnostic"),
        evidence: EvidenceByteLimit::new(4_096).expect("evidence"),
    }
}

fn byte_limit(value: u64) -> OutputByteLimit {
    OutputByteLimit::new(value).expect("positive output limit")
}

fn environment_id() -> ContentId<ExecutionEnvironmentArtifact> {
    id::<ExecutionEnvironmentArtifact>(b"reduction environment")
}

fn stripped_fixture(path: &str) -> Vec<u8> {
    let directory = tempfile::tempdir().expect("strip directory");
    let output = directory.path().join("fixture");
    let status = Command::new("strip")
        .args(["--strip-all", "-o"])
        .arg(&output)
        .arg(path)
        .status()
        .expect("GNU strip");
    assert!(status.success());
    fs::read(output).expect("stripped fixture")
}

fn open_content(directory: &tempfile::TempDir) -> SqliteContentStore {
    SqliteContentStore::open(
        directory.path().join("content.db"),
        directory.path().join("cas"),
    )
    .expect("content store")
}

fn open_events(directory: &tempfile::TempDir) -> SqliteEventStore {
    SqliteEventStore::open(directory.path().join("events.db")).expect("event store")
}

fn put<T: ContentType>(content: &mut SqliteContentStore, bytes: &[u8]) -> ContentId<T> {
    content
        .put::<T>(&mut Cursor::new(bytes))
        .expect("archive content")
        .content_id
}

fn id<T: ContentType>(bytes: &[u8]) -> ContentId<T> {
    ContentId::derive(bytes).expect("content identity")
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "Result::map_err supplies an owned I/O error"
)]
fn not_started(error: std::io::Error) -> ExecutorError {
    ExecutorError::NotStarted(error.to_string())
}
