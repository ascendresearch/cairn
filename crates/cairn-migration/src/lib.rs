//! Operator-migration product semantics and translation into domain-neutral execution requests.

#[cfg(feature = "agent-runtime")]
mod agent_materials;
mod assemble;
mod call_adapter;
mod candidate_admission;
mod candidate_build;
mod candidate_exploration;
mod collection_oracle;
mod controller_workflow;
mod corpus_execution;
mod corpus_observation;
mod domain;
mod exact_comparison;
mod executable_oracle;
#[cfg(feature = "agent-runtime")]
mod external_research;
mod historical;
mod input_values;
mod intent_admission;
mod intent_claim;
mod intent_promotion;
mod materialize;
mod memory_surface;
mod oracle_control;
mod oracle_exploration;
mod reduction_admission;
mod reduction_candidate;
mod reduction_control;
mod reduction_mutation;
#[cfg(feature = "agent-runtime")]
mod role_agent;
mod sir;
mod sir_contract;
mod variant_execution;
#[cfg(feature = "agent-runtime")]
pub use agent_materials::{
    OracleAgentBuildTestsV1, OracleAgentContextError, OracleAgentDocumentationV1,
    OracleAgentKnowledgeV1, OracleAgentMaterialsV1,
};

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
    prepare_boundary_call_adapter_input, prepare_collection_output_call_adapter_input,
    prepare_executable_oracle_call_adapter_input, prepare_input_value_call_adapter_input,
    prepare_memory_surface_call_adapter_input, validate_boundary_call_adapter_capture,
    validate_boundary_call_adapter_receipt, validate_collection_output_call_adapter_capture,
    validate_collection_output_call_adapter_receipt,
    validate_executable_oracle_call_adapter_capture,
    validate_executable_oracle_call_adapter_receipt, validate_input_value_call_adapter_capture,
    validate_input_value_call_adapter_receipt, validate_memory_surface_call_adapter_capture,
    validate_memory_surface_call_adapter_receipt,
};
pub use candidate_admission::{
    CandidateAdmissionAttemptArtifact, CandidateAdmissionAttemptV1, CandidateAdmissionError,
    CandidateAdmissionEvidenceArtifact, CandidateAdmissionEvidenceV1,
    CandidateAdmissionOutcomeArtifact, CandidateAdmissionOutcomeV1, CandidateClaimOutcomeV1,
    CandidateClaimStatusV1, CandidateControlFamilyV1, CandidateControlImplementationArtifact,
    CandidateControlObligationV1, CandidateControlReceiptV1, CandidateControlResultV1,
    CandidateMechanismCatalogArtifact, CandidateMechanismCatalogV1, CandidateMechanismProvenanceV1,
    CandidateQualifiedMechanismArtifact, CandidateQualifiedMechanismV1,
    TrustedCandidateControlReceiptArtifact, recompute_candidate_admission,
};
pub use candidate_build::{
    CandidateBuildError, CandidateBuildPlanArtifact, CandidateBuildPlanV1,
    CandidateBuildRequestArtifact, CandidateBuildRequestV1, PreparedGenericCandidateBuildJob,
    prepare_generic_candidate_build_job,
};
pub use candidate_exploration::{
    CandidateAdmittedOracleClaimV1, CandidateExplanation, CandidateExplorationError,
    CandidateOracleContractArtifact, CandidateOracleContractV1, CandidateOracleElementMaterialV1,
    CandidateOracleMaterialV1, CandidateOracleMaterialsV1, CandidateProposalArtifact,
    CandidateProposalSubmissionV1, CandidateProposalV1, CandidateSourceFileV1, CandidateSourcePath,
    CandidateSourceText, CandidateWorkspaceArtifact, CandidateWorkspaceV1,
};
pub use collection_oracle::{
    AssembledCollectionF32OracleCaseInput, CollectionF32Bits, CollectionF32InputBufferV1,
    CollectionF32InputBytesArtifact, CollectionF32InvocationArtifact, CollectionF32InvocationV1,
    CollectionF32OutputBufferV1, CollectionF32ThresholdBytesArtifact, CollectionF32ThresholdV1,
    CollectionOracleElementArtifact, CollectionOracleMechanismArtifact,
    CollectionOutputComparisonEvidenceArtifact, CollectionOutputComparisonEvidenceV1,
    CollectionOutputComparisonV1, CollectionOutputOracleDecisionArtifact,
    CollectionOutputOracleDecisionV1, CollectionOutputOracleError, CollectionOutputOraclePolicyV1,
    CollectionReportedCount, ExpectedCollectionOracleOutputArtifact,
    ExpectedCollectionOracleOutputV1, MigrationIntentContractArtifact,
    ObservedCollectionOracleOutputArtifact, ObservedCollectionOracleOutputV1,
    PreparedCollectionOutputComparisonEvidence, assemble_collection_f32_oracle_case,
    collection_oracle_mechanism_id, materialize_collection_output_comparison,
};
pub use controller_workflow::{
    CandidateAdmissionDispositionV1, CudaMigrationWorkflow, OracleAdmissionDispositionV1,
    run_cuda_migration,
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
#[cfg(feature = "agent-runtime")]
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
pub use intent_admission::{
    IntentAdmissionError, IntentDecisionRequestBatchArtifact, IntentDecisionRequestBatchV1,
    UserIntentCallerUnknownContextV1, UserIntentDecisionOptionV1,
    UserIntentDecisionRequestArtifact, UserIntentDecisionRequestV1, UserIntentDecisionResponseKind,
    derive_user_intent_decision_requests,
};
pub use intent_claim::{AuthoritativeIntentClaimV1, OperationIntentV1};
pub use intent_promotion::{
    IntentAdmissionPublicOutcomeArtifact, IntentAdmissionPublicOutcomeV1, IntentPromotionError,
    IntentUserDecisionGateArtifact, MigrationIntentContractV1, PreparedIntentAdmissionV1,
    RestrictedIntentAdmissionDecisionArtifact, RestrictedIntentAdmissionDecisionV1,
    TaskIntentAuthoritySubject, UserIntentAuthorityGrantArtifact, UserIntentAuthorityGrantV1,
    UserIntentAuthorityScopeV1, UserIntentDecisionArtifact, UserIntentDecisionResponseV1,
    UserIntentDecisionV1, UserProvidedIntentClaimV1, intent_user_decision_gate_id,
    promote_user_intent,
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
pub use oracle_control::{
    OracleControlDispatchArtifact, OracleControlDispatchV1, OracleControlRunArtifact,
    OracleControlRunV1, OracleControlRunnerArtifact, OracleControlWorker,
    OracleControlWorkerBindingV1, OracleControlWorkerError,
    OracleMechanismQualificationReceiptArtifact, OracleMechanismQualificationReceiptV1,
    TrustedOracleControlObservationV1,
};
pub use oracle_exploration::{
    AgentLoopRuntimeBindingArtifact, IndependentOracleAdmissionStages,
    OracleAdmissionAttemptArtifact, OracleAdmissionAttemptV1, OracleAdmissionEvidenceArtifact,
    OracleAdmissionEvidenceV1, OracleAdmissionMechanismCatalogArtifact,
    OracleAdmissionMechanismCatalogV1, OracleAdmissionOutcomeArtifact, OracleAdmissionOutcomeV1,
    OracleAdmissionPolicyArtifact, OracleAdmissionPolicyV1, OracleAdversarialPolicyV1,
    OracleBuildTestSnapshotArtifact, OracleClaimAdmissionStatusV1, OracleClaimAdmissionV1,
    OracleClaimArtifact, OracleClaimName, OracleClaimV1, OracleComparatorProposalArtifact,
    OracleConcernV1, OracleControlFamilyV1, OracleControlObligationV1, OracleControlReceiptV1,
    OracleControlResultV1, OracleCoverageGapArtifact, OracleCoveragePolicyArtifact,
    OracleCoveragePolicyV1, OracleCoverageProfileV1, OracleDocumentationSnapshotArtifact,
    OracleExecutionSafetyProposalArtifact, OracleExperimentArgumentsArtifact,
    OracleExperimentLimit, OracleExperimentOperationName, OracleExperimentRequestArtifact,
    OracleExperimentRequestV1, OracleExperimentToolCatalogArtifact, OracleExplorationBudgetV1,
    OracleExplorationCapabilityGrantArtifact, OracleExplorationDirectiveV1,
    OracleExplorationLedgerArtifact, OracleExplorationLedgerV1, OracleExplorationNextActionV1,
    OracleExplorationObservationArtifact, OracleExplorationObservationV1,
    OracleExplorationRevision, OracleExplorationRunOutcomeV1, OracleExplorationStages,
    OracleFrameworkError, OracleKnowledgeSnapshotArtifact, OracleObligationEntryV1,
    OracleObligationResolutionV1, OracleObservationPayloadArtifact, OracleObservationPayloadV1,
    OracleObservationProvenanceV1, OraclePlaneV1, OraclePortfolioElementArtifact,
    OraclePortfolioElementKindV1, OraclePortfolioElementV1, OraclePortfolioProposalArtifact,
    OraclePortfolioProposalV1, OracleQualifiedMechanismArtifact,
    OracleQualifiedMechanismRegistrationV1, OracleResearchExchangeArtifact,
    OracleResearchToolCatalogArtifact, OracleSourceSnapshotArtifact, OracleStrategyCatalogArtifact,
    OracleStrategyCatalogV1, OracleStrategyExecutorV1, OracleStrategyImplementationArtifact,
    OracleStrategyKindV1, OracleStrategyName, OracleStrategyRegistrationV1, OracleStrategyRoleV1,
    OracleStrategyRunArtifact, OracleStrategyRunLimit, OracleStrategyRunV1,
    OracleStrategySubmissionArtifact, OracleStrategySubmissionOutcomeV1,
    OracleStrategySubmissionV1, OracleStrategyToolCatalogArtifact, OracleStrategyToolCatalogV1,
    OracleStrategyToolV1, OracleUnknownEvidenceArtifact, OracleUnknownEvidenceV1,
    OracleUnknownReason, OracleWaiverAuthorityArtifact, OracleWorkItemArtifact, OracleWorkItemV1,
    OracleWorkspaceArtifact, OracleWorkspaceInput, OracleWorkspaceV1,
    TrustedOracleControlReceiptArtifact, TrustedOracleWorkerReceiptArtifact,
    TrustedOracleWorkerReceiptV1, WorkflowToolControllerObservationArtifact,
    archive_oracle_framework_artifact, derive_oracle_claims, derive_oracle_work_items,
    recompute_oracle_admission, run_independent_oracle_admission, run_oracle_exploration,
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
#[cfg(feature = "agent-runtime")]
pub use role_agent::{
    CandidateExplorationAgentContextV1, CandidateExplorationRoleHooksV1,
    CandidateReviewAgentContextV1, CandidateReviewRoleHooksV1, CandidateRevisionAgentContextV1,
    CandidateRevisionRoleHooksV1, MigrationAgentRoleError, MigrationAgentToolV1,
    MigrationRoleHooksV1, MigrationRoleStepObservationV1, OracleExplorationAgentContextV1,
    OracleExplorationRoleHooksV1, OracleReviewAgentContextV1, OracleReviewRoleHooksV1,
    OracleRevisionAgentContextV1, OracleRevisionRoleHooksV1, SirAgentContextV1, SirRoleHooksV1,
};
#[cfg(feature = "agent-runtime")]
pub use sir::SirTaskWorkspace;
pub use sir::{
    SirError, SirIntentHypothesisSetProposalArtifact, SirReadByteLimit, SirReadLineLimit,
    SirSourceCitationV1, SirSourceLineCount, SirSourceLineNumber, SirTaskArtifactBytes,
    SirTaskArtifactPath, SirTaskArtifactV1, SirTaskBundleArtifact, SirTaskBundleV1,
    SirTaskByteLimit, SirTaskFileLimit, SirTaskLimits,
};
pub use sir_contract::{
    AgentResolvedRuntimeModelArtifact, IntentHypothesisSetProposalV1, IntentRecoveryInputArtifact,
    IntentRecoveryInputV1, IntentRecoveryRequestV1, SirArgumentName, SirAuthorizedEvidenceArtifact,
    SirCallerArgumentRole, SirCallerArgumentV1, SirCallerClaimId, SirCallerClaimStatement,
    SirCallerClaimV1, SirCallerDeclarationV1, SirCallerExclusionId, SirCallerExclusionV1,
    SirCallerReferenceArtifact, SirCapability, SirCapabilityManifestV1, SirConflictId,
    SirConflictStatement, SirDeclaredShapeV1, SirDeclaredUnknownId, SirDeclaredUnknownKind,
    SirDeclaredUnknownQuestion, SirDeclaredUnknownV1, SirDisambiguationExperimentV1,
    SirDisambiguationTargetV1, SirDispositionRationale, SirErrorBehaviorDeclaration,
    SirExclusionStatement, SirExperimentId, SirExperimentPlan, SirExperimentPrediction,
    SirHypothesisClaim, SirHypothesisId, SirIntentClaimRefV1, SirIntentConflictV1, SirIntentDomain,
    SirIntentEvidenceRefV1, SirIntentHypothesisV1, SirIntentLayer, SirInvariantId,
    SirInvariantStatement, SirObservationId, SirObservationStatement, SirObservedFactV1,
    SirOptimizationFreedomId, SirOptimizationFreedomStatement, SirOptimizationFreedomV1,
    SirPriorFeedbackArtifact, SirPriorFeedbackV1, SirProposalSubmissionV1, SirSemanticInvariantV1,
    SirShapeExpression, SirSourceBehaviorDispositionKind, SirSourceBehaviorDispositionV1,
    SirSourceDispositionId, SirTargetContextV1, SirTargetEnvironmentSelectionV1, SirTargetSoc,
    SirTargetSocSelectionV1, SirTargetToolchain, SirTargetToolchainSelectionV1, SirUnknownId,
    SirUnknownKind, SirUnknownQuestion, SirUnknownV1, SirValueDomainDeclaration,
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
