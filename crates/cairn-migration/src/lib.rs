//! Operator-migration product semantics and translation into domain-neutral execution requests.

mod assemble;
mod call_adapter;
mod corpus_execution;
mod corpus_observation;
mod domain;
mod exact_comparison;
mod executable_oracle;
mod external_research;
mod historical;
mod input_values;
mod materialize;
mod memory_surface;
mod oracle_prompt;
mod oracle_search;
mod oracle_tools;
mod oracle_workflow;
mod reduction_admission;
mod reduction_candidate;
mod reduction_control;
mod reduction_mutation;
mod sir;
mod variant_execution;

pub use assemble::{
    AssembledBoundaryCaseInput, AssembledInputValueCaseInput, AssembledMemorySurfaceCaseInput,
    CorpusCaseAssemblyError, MaterializedAbiArgumentV1, MaterializedBoundaryCaseArtifact,
    MaterializedBoundaryCaseV1, MaterializedInputValueCaseArtifact, MaterializedInputValueCaseV1,
    MaterializedMemorySurfaceCaseArtifact, MaterializedMemorySurfaceCaseV1,
    MaterializedScalarArgumentBytesArtifact, MemorySurfaceLayoutV1, assemble_boundary_case_input,
    assemble_input_value_case_input, assemble_memory_surface_case_input,
};
pub use call_adapter::{
    CallAdapterCaptureLimits, CallAdapterCompletionV1, CallAdapterExecutableArtifact,
    CallAdapterExecutableByteLimit, CallAdapterExpectedOutputV1, CallAdapterObservedOutputV1,
    CallAdapterOutputBytesArtifact, CallAdapterProtocolError, CallAdapterRequestArtifact,
    CallAdapterRequestV1, CallAdapterResultArtifact, CallAdapterResultV1,
    CorpusInvocationIdentityV1, PreparedCallAdapterInput, PreparedCallAdapterJob,
    ValidatedCallAdapterExecution, ValidatedCallAdapterObservation, compose_call_adapter_job,
    prepare_boundary_call_adapter_input, prepare_executable_oracle_call_adapter_input,
    prepare_input_value_call_adapter_input, prepare_memory_surface_call_adapter_input,
    validate_boundary_call_adapter_capture, validate_boundary_call_adapter_receipt,
    validate_executable_oracle_call_adapter_capture,
    validate_executable_oracle_call_adapter_receipt, validate_input_value_call_adapter_capture,
    validate_input_value_call_adapter_receipt, validate_memory_surface_call_adapter_capture,
    validate_memory_surface_call_adapter_receipt,
};
pub use corpus_execution::{
    AssembledCorpusExecutionCase, CorpusExecutionPlanArtifact, CorpusExecutionPlanError,
    CorpusExecutionPlanItemV1, CorpusExecutionPlanV1, CorpusExecutionSubjectV1,
    CorpusObligationIdentityV1, PreparedCorpusExecutionCase, PreparedCorpusExecutionPlan,
    prepare_corpus_execution_plan,
};
pub use corpus_observation::{
    CorpusExecutionReceipt, CorpusObservationItemV1, CorpusObservationSetArtifact,
    CorpusObservationSetError, CorpusObservationSetV1, ValidatedCorpusExecutionCase,
    ValidatedCorpusObservationSet, validate_corpus_execution_receipts,
};
pub use domain::{
    ArgumentIndex, BufferAccessV1, BufferContractInput, BufferContractV1, BufferName, BufferRole,
    CaseExpectedOutcome, CaseTarget, DataType, DimensionAxis, DimensionSpec, DomainContractError,
    EntryPointName, ExtentModulus, ExtentValue, InclusiveExtentRange, InclusiveIntegerRange,
    IntegerValue, InvalidInputBehavior, MandatoryCaseDerivationPolicy, MigrationDomainCaseArtifact,
    MigrationDomainCaseV1, MigrationDomainContractInput, MigrationDomainContractV1,
    MigrationDomainExclusionArtifact, MigrationMandatoryCasesArtifact, MigrationMandatoryCasesV1,
    RequestedSemanticsArtifact, ScalarAssignment, ScalarBoundaryObligation,
    ScalarParameterContractInput, ScalarParameterContractV1, ScalarParameterName,
    ScalarParameterRole, SemanticClaimKind, ShapeAssignment, ShapeBoundaryObligation, ShapeRank,
    ShapeSymbolContractInput, ShapeSymbolContractV1, ShapeSymbolName, ShapeSymbolSource,
    StatusCode, derive_mandatory_base_cases,
};
pub use exact_comparison::{
    ExactCaseComparisonV1, ExactCorpusComparisonArtifact, ExactCorpusComparisonError,
    ExactCorpusComparisonV1, ExactOutputComparisonV1, PreparedExactCorpusComparison,
    compare_exact_corpus_observations,
};
pub use executable_oracle::{
    AssembledExecutableOracleCaseInput, ExecutableOracleComparisonV1, ExecutableOracleError,
    ExecutableOracleInputBufferV1, ExecutableOracleInputBytesArtifact,
    ExecutableOracleInvocationArtifact, ExecutableOracleInvocationV1,
    ExecutableOracleOutputBufferV1, ExecutableOracleOutputComparisonArtifact,
    ExecutableOracleOutputComparisonV1, OracleF32Bits, OracleMatrixShapeV1,
    PreparedExecutableOracleOutputComparison, ZeroKMatmulF32OracleCaseV1,
    assemble_zero_k_matmul_f32_oracle, compare_executable_oracle_output,
};
pub use external_research::{
    ArchivedExternalTestEvidence, ExternalResearchPolicy, ExternalResearchProvider,
    ExternalResearchProviderError, ExternalTestCaseV1, ExternalTestResearchContextV1,
    ExternalTestResearchSnippetV1, ExternalTestSearchGateway, ExternalTestSearchRequestArtifact,
    ExternalTestSearchRequestV1, ExternalTestSearchResultArtifact, ExternalTestSearchResultV1,
    GitHubBlobIdentity, GitHubExternalResearchProvider, GitHubRepository,
    RecordedExternalResearchExchange, RecordedExternalResearchProvider, SearchQuery,
    SearchResultLimit, SourcePath, archive_external_test_evidence,
    external_test_search_registration,
};
pub use historical::{
    HistoricalDetectionRequirement, HistoricalDiagnosticClassName, HistoricalFailureClassName,
    HistoricalFailureContractError, HistoricalFailureCoverageArtifact, HistoricalFailureCoverageV1,
    HistoricalFailureEvidenceArtifact, HistoricalFailureObligationArtifact,
    HistoricalFailureObligationV1, HistoricalFailureRecordArtifact, HistoricalFailureRecordInput,
    HistoricalFailureRecordV1, HistoricalFailureScope, HistoricalObservationClassName,
    HistoricalObservedFailureArtifact, HistoricalReproductionArtifact, HistoricalValidationStage,
    MigrationDomainFamilyName, OracleFailureMechanismName, TargetMechanismName,
};
pub use input_values::{
    BooleanInputPattern, FloatingDataType, FloatingInputPattern, FloatingInputValueDomainInput,
    FloatingInputValueDomainV1, InputValueCaseTarget, InputValueDerivationPolicy,
    InputValueDisposition, InputValueDomainV1, MandatoryInputValueCaseArtifact,
    MandatoryInputValueCaseV1, MandatoryInputValueCasesArtifact, MandatoryInputValueCasesV1,
    SignedIntegerDataType, SignedIntegerInputPattern, UnsignedIntegerDataType,
    UnsignedIntegerInputPattern, derive_mandatory_input_value_cases,
};
pub use materialize::{
    CorpusBufferByteLength, CorpusBufferByteLimit, CorpusByteOrder, CorpusElementCount,
    CorpusMaterializationError, MaterializedCorpusBuffer, MaterializedCorpusBufferArtifact,
    MaterializedCorpusBufferBytesArtifact, MaterializedCorpusBufferV1,
    materialize_input_value_case,
};
pub use memory_surface::{
    BufferAliasingContractInput, BufferAliasingContractV1, BufferAliasingPattern,
    BufferMemoryContractInput, BufferMemoryContractV1, BufferMemoryPattern, BufferPairV1,
    CapacityShortfallBytes, MandatoryMemorySurfaceCaseArtifact, MandatoryMemorySurfaceCaseV1,
    MandatoryMemorySurfaceCasesArtifact, MandatoryMemorySurfaceCasesV1, MemoryConditionDisposition,
    MemorySurfaceCaseTarget, MemorySurfaceDerivationPolicy, MisalignmentOffsetBytes,
    PartialOverlapOffsetBytes, PointerAlignmentContractV1, RequiredAlignmentBytes,
    derive_mandatory_memory_surface_cases,
};
pub use oracle_prompt::{
    MaterializedOraclePrompt, OracleInstructionSetV1, OraclePromptError, OracleRolePromptArtifact,
    OracleRolePromptInput, OracleRolePromptV1, archive_oracle_role_prompt,
    archive_standard_oracle_instructions, materialize_oracle_prompt,
    oracle_common_instruction_text, oracle_role_instruction_text, prepare_oracle_role_prompt,
};
pub use oracle_search::{
    OracleAgentRole, OracleRoleEpisodeInput, OracleRoleEpisodeV1, OracleRoleTool,
    OracleSearchPlanArtifact, OracleSearchPlanError, OracleSearchPlanInput, OracleSearchPlanV1,
    archive_oracle_role_tool_catalog, oracle_role_tool_catalog_bytes, oracle_role_tool_catalog_id,
    prepare_oracle_role_episode,
};
pub use oracle_tools::{
    BlueDomainRefinementGateway, BlueProposalGateway, BlueProposalSubmissionV1, OracleToolError,
    RedSubmissionGateway, blue_domain_refinement_registration, blue_proposal_registration,
    oracle_role_native_tools, red_submission_registrations,
};
pub use oracle_workflow::{
    OracleAdmissionAttemptArtifact, OracleAdmissionFeedbackArtifact, OracleAdmissionFeedbackV1,
    OracleAttackArtifact, OracleAttackInput, OracleAttackV1, OracleDiagnosticEvidenceArtifact,
    OracleDiagnosticKind, OracleDiagnosticV1, OracleFeedbackTarget, OracleProposalRevisionArtifact,
    OracleProposalRevisionV1, OracleWorkflowError, PreparedOracleAdmissionFeedback,
    PreparedOracleAttack, PreparedOracleProposalRevision, prepare_oracle_admission_feedback,
    prepare_oracle_attack, prepare_oracle_proposal_revision,
};
pub use reduction_admission::{
    HistoricalReductionAdmissionInputs, PreparedHistoricalReductionAdmission,
    compose_historical_reduction_admission,
};
pub use reduction_candidate::{
    HistoricalReductionCandidateCaseV1, HistoricalReductionCandidateComparisonArtifact,
    HistoricalReductionCandidateComparisonV1, HistoricalReductionCandidateInputs,
    PreparedHistoricalReductionCandidateVerdict, compose_historical_reduction_candidate_verdict,
};
pub use reduction_control::{
    FiniteF32Bits, HistoricalReductionAlgorithm, HistoricalReductionCaptureLimits,
    HistoricalReductionCaseArtifact, HistoricalReductionCaseComparisonV1,
    HistoricalReductionCaseEntryV1, HistoricalReductionCaseOutputV1, HistoricalReductionCaseV1,
    HistoricalReductionControlArtifact, HistoricalReductionControlError,
    HistoricalReductionControlV1, HistoricalReductionCorpusArtifact, HistoricalReductionCorpusV1,
    HistoricalReductionCorrectVariantEvidence, HistoricalReductionExecutionPlanArtifact,
    HistoricalReductionExecutionPlanV1, HistoricalReductionExecutionReceiptArtifact,
    HistoricalReductionExecutionReceiptV1, HistoricalReductionExecutionSubjectV1,
    HistoricalReductionFixtureOutputArtifact, HistoricalReductionFixtureOutputV1,
    HistoricalReductionTrialExpectationV1, HistoricalReductionVariantTrialV1,
    HistoricalReductionWrongVariantEvidence, PreparedHistoricalReductionControl,
    PreparedHistoricalReductionCorpus, PreparedHistoricalReductionJob, ReductionUlpDistance,
    ValidatedHistoricalReductionRun, compose_historical_reduction_control,
    compute_historical_reduction_fixture_output, compute_historical_reduction_output,
    prepare_historical_reduction_candidate_job, prepare_historical_reduction_corpus,
    prepare_historical_reduction_reference_job, prepare_historical_reduction_variant_job,
    validate_historical_reduction_receipt,
};
pub use reduction_mutation::{
    HistoricalReductionMutationCaseComparisonArtifact, HistoricalReductionMutationCaseComparisonV1,
    HistoricalReductionMutationInjectionArtifact, HistoricalReductionMutationInjectionV1,
    HistoricalReductionMutationInputs, HistoricalReductionMutationKind,
    HistoricalReductionMutationVariantEvidence, PreparedHistoricalReductionMutationGrid,
    compose_historical_reduction_mutation_grid, prepare_historical_reduction_mutant_set,
};
pub use sir::{
    IntentHypothesisSetProposalV1, SirCitedFactV1, SirEpisodeRunError, SirEpisodeRunInput,
    SirEpisodeRunOutcome, SirError, SirFactStatement, SirHypothesisSummary,
    SirIntentHypothesisSetProposalArtifact, SirIntentHypothesisV1, SirProposalSubmissionV1,
    SirReadByteLimit, SirReadLineLimit, SirSourceCitationV1, SirSourceLineCount,
    SirSourceLineNumber, SirTaskArtifactBytes, SirTaskArtifactPath, SirTaskArtifactV1,
    SirTaskBundleArtifact, SirTaskBundleV1, SirTaskByteLimit, SirTaskFileLimit, SirTaskLimits,
    SirTaskWorkspace, SirUnknownQuestion, SirUnknownV1, run_sir_episode,
};
pub use variant_execution::{
    ExactVariantTrialArtifact, ExactVariantTrialV1, PreparedExactVariantTrial,
    PreparedVariantBuildJob, ValidatedVariantBuild, VariantBuildCaptureLimits,
    VariantBuildDriverArtifact, VariantBuildDriverByteLimit, VariantBuildPlanArtifact,
    VariantBuildPlanV1, VariantBuildReceiptArtifact, VariantBuildReceiptV1, VariantExecutionError,
    VariantImplementationByteLimit, compose_exact_variant_trial, prepare_variant_build_job,
    validate_variant_build_receipt,
};

use cairn_execution::{
    ArchitectureName, CapabilityRequirement, ContractValueError, ExecutionBackend,
    ExecutionPlatformRequirement, ExecutionTimeoutMillis, OperatingSystemName, PlacementRequest,
    ResourceRequest, TargetEnvironmentName, WorkerPoolName,
};
use serde::{Deserialize, Serialize};

/// Product-owned validation position. This value must never be copied into worker profiles or
/// generic execution records.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MigrationValidationTier {
    /// Schema, reference, corpus, properties, and comparator checks on CPU capacity.
    V0Cpu,
    /// Observed source behavior on a source accelerator.
    V1SourceAccelerator,
    /// Target compilation, linkage, and ABI checks.
    V2TargetBuild,
    /// Target-device behavior and candidate-verdict evidence.
    V3TargetDevice,
}

/// One migration-stage execution need before crossing the generic scheduler boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationExecutionNeed {
    tier: MigrationValidationTier,
    backend: ExecutionBackend,
    timeout: ExecutionTimeoutMillis,
    architecture: Option<ArchitectureName>,
    operating_system: Option<OperatingSystemName>,
    target_environment: Option<TargetEnvironmentName>,
    allowed_worker_pools: Vec<WorkerPoolName>,
    capabilities: Vec<CapabilityRequirement>,
}

impl MigrationExecutionNeed {
    /// Creates a product-owned execution need and canonicalizes its generic selectors.
    ///
    /// # Errors
    ///
    /// Rejects duplicate pool or capability selectors.
    #[expect(
        clippy::too_many_arguments,
        reason = "product intent keeps every independent execution constraint explicit"
    )]
    pub fn new(
        tier: MigrationValidationTier,
        backend: ExecutionBackend,
        timeout: ExecutionTimeoutMillis,
        architecture: Option<ArchitectureName>,
        operating_system: Option<OperatingSystemName>,
        target_environment: Option<TargetEnvironmentName>,
        allowed_worker_pools: Vec<WorkerPoolName>,
        capabilities: Vec<CapabilityRequirement>,
    ) -> Result<Self, ContractValueError> {
        let placement = PlacementRequest::new(
            ExecutionPlatformRequirement::new(
                architecture.clone(),
                operating_system.clone(),
                target_environment.clone(),
            ),
            allowed_worker_pools,
            capabilities,
        )?;
        Ok(Self {
            tier,
            backend,
            timeout,
            architecture,
            operating_system,
            target_environment,
            allowed_worker_pools: placement.allowed_worker_pools().to_vec(),
            capabilities: placement.capabilities().to_vec(),
        })
    }

    /// Returns the product validation tier retained by migration orchestration.
    #[must_use]
    pub const fn tier(&self) -> MigrationValidationTier {
        self.tier
    }

    /// Returns the domain-neutral backend placed into the opaque job contract.
    #[must_use]
    pub const fn backend(&self) -> &ExecutionBackend {
        &self.backend
    }

    /// Translates product intent into the complete generic scheduler constraint.
    ///
    /// The migration tier is deliberately absent from the returned value.
    ///
    /// # Errors
    ///
    /// Returns an error only if deserialized state bypassed constructor invariants.
    pub fn to_resource_request(&self) -> Result<ResourceRequest, ContractValueError> {
        ResourceRequest::new(
            self.timeout,
            PlacementRequest::new(
                ExecutionPlatformRequirement::new(
                    self.architecture.clone(),
                    self.operating_system.clone(),
                    self.target_environment.clone(),
                ),
                self.allowed_worker_pools.clone(),
                self.capabilities.clone(),
            )?,
        )
    }
}

#[cfg(test)]
mod tests {
    use cairn_execution::{
        ArchitectureName, CapabilityName, CapabilityRequirement, CapabilityValue, ExecutionBackend,
        ExecutionTimeoutMillis, OperatingSystemName, TargetEnvironmentName, WorkerPoolName,
    };

    use super::{MigrationExecutionNeed, MigrationValidationTier};

    #[test]
    fn migration_tier_translates_without_crossing_execution_boundary() {
        let need = MigrationExecutionNeed::new(
            MigrationValidationTier::V3TargetDevice,
            ExecutionBackend::new("container").expect("backend"),
            ExecutionTimeoutMillis::new(30_000).expect("timeout"),
            Some(ArchitectureName::new("aarch64").expect("architecture")),
            Some(OperatingSystemName::new("linux").expect("operating system")),
            Some(TargetEnvironmentName::new("gnu").expect("target environment")),
            vec![WorkerPoolName::new("target-lab").expect("pool")],
            vec![CapabilityRequirement {
                name: CapabilityName::new("device-family").expect("capability"),
                value: CapabilityValue::new("fixture-device").expect("value"),
            }],
        )
        .expect("migration need");

        let resources = need.to_resource_request().expect("resource request");
        let placement = resources.placement();
        assert_eq!(
            placement
                .platform()
                .architecture()
                .expect("architecture")
                .as_str(),
            "aarch64"
        );
        assert_eq!(placement.allowed_worker_pools()[0].as_str(), "target-lab");
        assert_eq!(placement.capabilities()[0].name.as_str(), "device-family");
        let wire = serde_json::to_string(placement).expect("generic placement wire");
        assert!(!wire.contains("target-device"));
        assert!(!wire.contains("migration"));
        assert!(!wire.contains("v3"));
    }
}
