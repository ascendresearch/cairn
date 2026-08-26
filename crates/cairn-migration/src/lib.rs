//! Operator-migration product semantics and translation into domain-neutral execution requests.

mod assemble;
mod domain;
mod historical;
mod input_values;
mod materialize;
mod memory_surface;

pub use assemble::{
    AssembledBoundaryCaseInput, AssembledInputValueCaseInput, CorpusCaseAssemblyError,
    MaterializedAbiArgumentV1, MaterializedBoundaryCaseArtifact, MaterializedBoundaryCaseV1,
    MaterializedInputValueCaseArtifact, MaterializedInputValueCaseV1,
    MaterializedScalarArgumentBytesArtifact, assemble_boundary_case_input,
    assemble_input_value_case_input,
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
