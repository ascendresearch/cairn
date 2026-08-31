//! Claim-scoped, multi-plane Oracle Exploration and independent admission kernel.
//!
//! Completeness is represented by controller-derived obligations. A strategy, including a model
//! episode, may contribute material or preserve an unknown, but it cannot remove an obligation or
//! declare its own proposal admitted.

#![allow(
    clippy::missing_errors_doc,
    reason = "fallible Oracle APIs return the explicit framework or stage-owned error unchanged"
)]

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::io::Cursor;

use cairn_execution::{ExecutionReceiptArtifact, JobContractArtifact};
use cairn_protocol::{ContentId, ContentType, TaskId};
use cairn_record::{ContentStore, ContentStoreError};
use cairn_verification::{
    CorpusCaseArtifact, DomainRefinementArtifact, ModelConfigurationArtifact,
    ObservationPlanArtifact, PropertyRelationArtifact, ReferenceArtifact,
    SourceAdmissionPlanArtifact, ValidFamilyPlanArtifact,
};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    AuthoritativeIntentClaimV1, IntentRecoveryInputArtifact, MigrationIntentContractArtifact,
    OracleControlRunV1, OracleControlRunnerArtifact, OracleMechanismQualificationReceiptArtifact,
    SirTaskBundleArtifact, TrustedOracleControlObservationV1,
};

const SCHEMA_V1: u16 = 1;

macro_rules! artifact {
    ($(#[$meta:meta])* $name:ident, $domain:literal) => {
        $(#[$meta])*
        pub enum $name {}

        impl ContentType for $name {
            const DOMAIN: &'static str = $domain;
        }
    };
}

artifact!(
    /// Exact role-scoped Agent Loop runtime binding cited by an Agent-backed Oracle strategy.
    AgentLoopRuntimeBindingArtifact,
    "agent.loop-runtime-binding.v1"
);

artifact!(OracleClaimArtifact, "migration.oracle-claim.v1");
artifact!(
    WorkflowToolControllerObservationArtifact,
    "migration.workflow-tool-controller-observation.v1"
);
artifact!(
    OracleCoveragePolicyArtifact,
    "migration.oracle-coverage-policy.v1"
);
artifact!(
    OracleStrategyCatalogArtifact,
    "migration.oracle-strategy-catalog.v1"
);
artifact!(
    OracleSourceSnapshotArtifact,
    "migration.oracle-source-snapshot.v1"
);
artifact!(
    OracleDocumentationSnapshotArtifact,
    "migration.oracle-documentation-snapshot.v1"
);
artifact!(
    OracleBuildTestSnapshotArtifact,
    "migration.oracle-build-test-snapshot.v1"
);
artifact!(
    OracleKnowledgeSnapshotArtifact,
    "migration.oracle-knowledge-snapshot.v1"
);
artifact!(
    OracleResearchToolCatalogArtifact,
    "migration.oracle-research-tool-catalog.v1"
);
artifact!(
    OracleExperimentToolCatalogArtifact,
    "migration.oracle-experiment-tool-catalog.v1"
);
artifact!(
    OracleExplorationCapabilityGrantArtifact,
    "migration.oracle-exploration-capability-grant.v1"
);
artifact!(
    OracleStrategyImplementationArtifact,
    "migration.oracle-strategy-implementation.v1"
);
artifact!(
    OracleStrategyToolCatalogArtifact,
    "migration.oracle-strategy-tool-catalog.v1"
);
artifact!(OracleWorkspaceArtifact, "migration.oracle-workspace.v1");
artifact!(OracleWorkItemArtifact, "migration.oracle-work-item.v1");
artifact!(
    OracleStrategyRunArtifact,
    "migration.oracle-strategy-run.v1"
);
artifact!(
    OracleStrategySubmissionArtifact,
    "migration.oracle-strategy-submission.v1"
);
artifact!(
    OracleExperimentArgumentsArtifact,
    "migration.oracle-experiment-arguments.v1"
);
artifact!(
    OraclePortfolioElementArtifact,
    "migration.oracle-portfolio-element.v1"
);
artifact!(
    OracleComparatorProposalArtifact,
    "migration.oracle-comparator-proposal.v1"
);
artifact!(
    OracleExecutionSafetyProposalArtifact,
    "migration.oracle-execution-safety-proposal.v1"
);
artifact!(
    OracleCoverageGapArtifact,
    "migration.oracle-coverage-gap.v1"
);
artifact!(
    OracleExperimentRequestArtifact,
    "migration.oracle-experiment-request.v1"
);
artifact!(
    OracleExplorationObservationArtifact,
    "migration.oracle-exploration-observation.v1"
);
artifact!(
    OracleObservationPayloadArtifact,
    "migration.oracle-observation-payload.v1"
);
artifact!(
    OracleResearchExchangeArtifact,
    "migration.oracle-research-exchange.v1"
);
artifact!(
    TrustedOracleWorkerReceiptArtifact,
    "migration.oracle-worker-receipt-trusted.v1"
);
artifact!(
    OracleUnknownEvidenceArtifact,
    "migration.oracle-unknown-evidence.v1"
);
artifact!(
    OracleWaiverAuthorityArtifact,
    "migration.oracle-waiver-authority.v1"
);
artifact!(
    OracleExplorationLedgerArtifact,
    "migration.oracle-exploration-ledger.v1"
);
artifact!(
    OraclePortfolioProposalArtifact,
    "migration.oracle-portfolio-proposal.v1"
);
artifact!(
    OracleAdmissionPolicyArtifact,
    "migration.oracle-admission-policy.v1"
);
artifact!(
    OracleAdmissionMechanismCatalogArtifact,
    "migration.oracle-admission-mechanism-catalog.v1"
);
artifact!(
    OracleAdmissionAttemptArtifact,
    "migration.oracle-admission-attempt.v1"
);
artifact!(
    OracleAdmissionEvidenceArtifact,
    "migration.oracle-admission-evidence.v1"
);
artifact!(
    OracleQualifiedMechanismArtifact,
    "migration.oracle-qualified-mechanism.v1"
);
artifact!(
    TrustedOracleControlReceiptArtifact,
    "migration.oracle-control-receipt-trusted.v1"
);
artifact!(
    OracleAdmissionOutcomeArtifact,
    "migration.oracle-admission-outcome.v1"
);

macro_rules! label {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a canonical task-generic label.
            pub fn new(value: impl Into<String>) -> Result<Self, OracleFrameworkError> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > 128
                    || !value.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || matches!(byte, b'-' | b'.' | b'_' | b'/')
                    })
                {
                    return Err(OracleFrameworkError::InvalidLabel($kind));
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

label!(OracleClaimName, "oracle claim name");
label!(OracleStrategyName, "oracle strategy name");
label!(
    OracleExperimentOperationName,
    "oracle experiment operation name"
);
label!(OracleUnknownReason, "oracle unknown reason");

/// One admitted-intent claim expanded independently by the Controller.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleClaimV1 {
    schema_version: u16,
    task_id: TaskId,
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    name: OracleClaimName,
    specification: AuthoritativeIntentClaimV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleClaimWire {
    schema_version: u16,
    task_id: TaskId,
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    name: OracleClaimName,
    specification: AuthoritativeIntentClaimV1,
}

impl OracleClaimV1 {
    #[must_use]
    pub fn new(
        task_id: TaskId,
        admitted_intent: ContentId<MigrationIntentContractArtifact>,
        name: OracleClaimName,
        specification: AuthoritativeIntentClaimV1,
    ) -> Self {
        Self {
            schema_version: SCHEMA_V1,
            task_id,
            admitted_intent,
            name,
            specification,
        }
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn admitted_intent(&self) -> ContentId<MigrationIntentContractArtifact> {
        self.admitted_intent
    }

    #[must_use]
    pub const fn name(&self) -> &OracleClaimName {
        &self.name
    }

    #[must_use]
    pub const fn specification(&self) -> &AuthoritativeIntentClaimV1 {
        &self.specification
    }

    pub fn identity(&self) -> Result<ContentId<OracleClaimArtifact>, OracleFrameworkError> {
        derive_id(self)
    }
}

impl TryFrom<OracleClaimWire> for OracleClaimV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OracleClaimWire) -> Result<Self, Self::Error> {
        require_v1(wire.schema_version)?;
        Ok(Self::new(
            wire.task_id,
            wire.admitted_intent,
            wire.name,
            wire.specification,
        ))
    }
}

impl<'de> Deserialize<'de> for OracleClaimV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleClaimWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Derives the complete current-V1 Oracle claim inventory from one admitted intent contract.
///
/// A current-V1 contract contains exactly one authoritative admitted claim. The claim remains
/// bound to the complete contract identity; its planes and concerns are expanded separately by
/// [`derive_oracle_work_items`]. Callers cannot supply or remove claims.
#[must_use]
pub fn derive_oracle_claims(
    task_id: TaskId,
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    admitted_claim: &AuthoritativeIntentClaimV1,
) -> Vec<OracleClaimV1> {
    vec![OracleClaimV1::new(
        task_id,
        admitted_intent,
        OracleClaimName("admitted-intent".to_owned()),
        admitted_claim.clone(),
    )]
}

/// Stable top-level plane. Concerns, rather than prompts, determine its required coverage.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OraclePlaneV1 {
    ObservableSemantics,
    InputDomain,
    NumericalBehavior,
    InterfaceStructure,
    StateMemoryEffects,
    FailureRejection,
    ConcurrencyDeterminism,
    ResourcePerformance,
    CrossPlaneInteraction,
    CoverageDiscovery,
}

/// Closed current-V1 coverage points mechanically expanded for every claim.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleConcernV1 {
    ObservableOutputs,
    AllowedResultRelations,
    ValidInputFamilies,
    BoundaryAndDegenerateInputs,
    InvalidInputBehavior,
    SpecialNumericValues,
    ToleranceAndStability,
    ShapeRankLayoutAndType,
    AliasingAndOverlap,
    MemoryWritesAndInitialization,
    LaunchFallbackAndExternalEffects,
    ErrorStatusAndRejection,
    OrderingRacesAndDeterminism,
    ResourceLimits,
    PerformanceEnvelope,
    CrossPlaneInvariants,
    UncataloguedRiskDiscovery,
}

impl OracleConcernV1 {
    #[must_use]
    pub const fn plane(self) -> OraclePlaneV1 {
        match self {
            Self::ObservableOutputs | Self::AllowedResultRelations => {
                OraclePlaneV1::ObservableSemantics
            }
            Self::ValidInputFamilies
            | Self::BoundaryAndDegenerateInputs
            | Self::InvalidInputBehavior => OraclePlaneV1::InputDomain,
            Self::SpecialNumericValues | Self::ToleranceAndStability => {
                OraclePlaneV1::NumericalBehavior
            }
            Self::ShapeRankLayoutAndType | Self::AliasingAndOverlap => {
                OraclePlaneV1::InterfaceStructure
            }
            Self::MemoryWritesAndInitialization | Self::LaunchFallbackAndExternalEffects => {
                OraclePlaneV1::StateMemoryEffects
            }
            Self::ErrorStatusAndRejection => OraclePlaneV1::FailureRejection,
            Self::OrderingRacesAndDeterminism => OraclePlaneV1::ConcurrencyDeterminism,
            Self::ResourceLimits | Self::PerformanceEnvelope => OraclePlaneV1::ResourcePerformance,
            Self::CrossPlaneInvariants => OraclePlaneV1::CrossPlaneInteraction,
            Self::UncataloguedRiskDiscovery => OraclePlaneV1::CoverageDiscovery,
        }
    }
}

const CORRECTNESS_CONCERNS: &[OracleConcernV1] = &[
    OracleConcernV1::ObservableOutputs,
    OracleConcernV1::AllowedResultRelations,
    OracleConcernV1::ValidInputFamilies,
    OracleConcernV1::BoundaryAndDegenerateInputs,
    OracleConcernV1::InvalidInputBehavior,
    OracleConcernV1::SpecialNumericValues,
    OracleConcernV1::ToleranceAndStability,
    OracleConcernV1::ShapeRankLayoutAndType,
    OracleConcernV1::AliasingAndOverlap,
    OracleConcernV1::MemoryWritesAndInitialization,
    OracleConcernV1::LaunchFallbackAndExternalEffects,
    OracleConcernV1::ErrorStatusAndRejection,
    OracleConcernV1::OrderingRacesAndDeterminism,
    OracleConcernV1::CrossPlaneInvariants,
    OracleConcernV1::UncataloguedRiskDiscovery,
];

const PERFORMANCE_CONCERNS: &[OracleConcernV1] = &[
    OracleConcernV1::ResourceLimits,
    OracleConcernV1::PerformanceEnvelope,
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleCoverageProfileV1 {
    Correctness,
    CorrectnessAndPerformance,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleAdversarialPolicyV1 {
    NotRequired,
    RequiredForEveryConcern,
}

/// Logical function required from a strategy. It does not imply an Agent or process topology.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleStrategyRoleV1 {
    Synthesis,
    Adversarial,
}

/// Controller-owned policy whose constructor derives, rather than accepts, mandatory concerns.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleCoveragePolicyV1 {
    schema_version: u16,
    profile: OracleCoverageProfileV1,
    adversarial: OracleAdversarialPolicyV1,
    concerns: Vec<OracleConcernV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleCoveragePolicyWire {
    schema_version: u16,
    profile: OracleCoverageProfileV1,
    adversarial: OracleAdversarialPolicyV1,
    concerns: Vec<OracleConcernV1>,
}

impl OracleCoveragePolicyV1 {
    #[must_use]
    pub fn new(profile: OracleCoverageProfileV1, adversarial: OracleAdversarialPolicyV1) -> Self {
        let mut concerns = CORRECTNESS_CONCERNS.to_vec();
        if profile == OracleCoverageProfileV1::CorrectnessAndPerformance {
            concerns.extend_from_slice(PERFORMANCE_CONCERNS);
        }
        concerns.sort_unstable();
        Self {
            schema_version: SCHEMA_V1,
            profile,
            adversarial,
            concerns,
        }
    }

    #[must_use]
    pub fn concerns(&self) -> &[OracleConcernV1] {
        &self.concerns
    }

    #[must_use]
    pub const fn adversarial(&self) -> OracleAdversarialPolicyV1 {
        self.adversarial
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleCoveragePolicyArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        let expected = Self::new(self.profile, self.adversarial);
        if self.concerns != expected.concerns {
            return Err(OracleFrameworkError::CoveragePolicyDrift);
        }
        Ok(())
    }
}

impl TryFrom<OracleCoveragePolicyWire> for OracleCoveragePolicyV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OracleCoveragePolicyWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            profile: wire.profile,
            adversarial: wire.adversarial,
            concerns: wire.concerns,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleCoveragePolicyV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleCoveragePolicyWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleStrategyKindV1 {
    DeterministicAnalyzer,
    ModelBackedSynthesis,
    ModelBackedAdversarial,
    Generator,
    Mutation,
    Property,
    Fuzz,
    CounterexampleSearch,
}

/// Exact implementation boundary for one registered strategy.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "executor", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OracleStrategyExecutorV1 {
    Deterministic {
        implementation: ContentId<OracleStrategyImplementationArtifact>,
    },
    AgentLoop {
        /// Verification-domain authorship identity retained on proposed Oracle material.
        authorship_model: ContentId<ModelConfigurationArtifact>,
        /// Exact resolved model, hook profile, budget, and transport binding consumed by the loop.
        invocation: ContentId<AgentLoopRuntimeBindingArtifact>,
        tools: ContentId<OracleStrategyToolCatalogArtifact>,
    },
}

/// Closed capabilities exposed to every current-V1 Agent-backed Oracle cell.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleStrategyToolV1 {
    ReadTaskArtifact,
    SearchExternalTests,
    RequestExperiment,
    SubmitCellResult,
}

/// Canonical current-V1 tool surface exposed to one Oracle strategy Agent Loop.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleStrategyToolCatalogV1 {
    schema_version: u16,
    tools: Vec<OracleStrategyToolV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleStrategyToolCatalogWire {
    schema_version: u16,
    tools: Vec<OracleStrategyToolV1>,
}

impl OracleStrategyToolCatalogV1 {
    #[must_use]
    pub fn standard() -> Self {
        Self {
            schema_version: SCHEMA_V1,
            tools: vec![
                OracleStrategyToolV1::ReadTaskArtifact,
                OracleStrategyToolV1::SearchExternalTests,
                OracleStrategyToolV1::RequestExperiment,
                OracleStrategyToolV1::SubmitCellResult,
            ],
        }
    }

    #[must_use]
    pub fn tools(&self) -> &[OracleStrategyToolV1] {
        &self.tools
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleStrategyToolCatalogArtifact>, OracleFrameworkError> {
        derive_id(self)
    }
}

impl TryFrom<OracleStrategyToolCatalogWire> for OracleStrategyToolCatalogV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleStrategyToolCatalogWire) -> Result<Self, Self::Error> {
        require_v1(wire.schema_version)?;
        let expected = Self::standard();
        if wire.tools != expected.tools {
            return Err(OracleFrameworkError::StrategyToolCatalogDrift);
        }
        Ok(expected)
    }
}

impl<'de> Deserialize<'de> for OracleStrategyToolCatalogV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleStrategyToolCatalogWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleStrategyRegistrationV1 {
    name: OracleStrategyName,
    kind: OracleStrategyKindV1,
    executor: OracleStrategyExecutorV1,
    roles: Vec<OracleStrategyRoleV1>,
    concerns: Vec<OracleConcernV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleStrategyRegistrationWire {
    name: OracleStrategyName,
    kind: OracleStrategyKindV1,
    executor: OracleStrategyExecutorV1,
    roles: Vec<OracleStrategyRoleV1>,
    concerns: Vec<OracleConcernV1>,
}

impl OracleStrategyRegistrationV1 {
    pub fn new(
        name: OracleStrategyName,
        kind: OracleStrategyKindV1,
        executor: OracleStrategyExecutorV1,
        roles: Vec<OracleStrategyRoleV1>,
        concerns: Vec<OracleConcernV1>,
    ) -> Result<Self, OracleFrameworkError> {
        validate_strict(&roles, "strategy roles")?;
        validate_strict(&concerns, "strategy concerns")?;
        match (&executor, kind) {
            (
                OracleStrategyExecutorV1::AgentLoop { .. },
                OracleStrategyKindV1::DeterministicAnalyzer,
            )
            | (
                OracleStrategyExecutorV1::Deterministic { .. },
                OracleStrategyKindV1::ModelBackedSynthesis
                | OracleStrategyKindV1::ModelBackedAdversarial,
            ) => return Err(OracleFrameworkError::StrategyExecutorMismatch),
            _ => {}
        }
        Ok(Self {
            name,
            kind,
            executor,
            roles,
            concerns,
        })
    }

    #[must_use]
    pub const fn name(&self) -> &OracleStrategyName {
        &self.name
    }

    #[must_use]
    pub const fn kind(&self) -> OracleStrategyKindV1 {
        self.kind
    }

    #[must_use]
    pub const fn executor(&self) -> &OracleStrategyExecutorV1 {
        &self.executor
    }

    fn supports(&self, item: &OracleWorkItemV1) -> bool {
        self.roles.contains(&item.role) && self.concerns.contains(&item.concern)
    }
}

impl TryFrom<OracleStrategyRegistrationWire> for OracleStrategyRegistrationV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OracleStrategyRegistrationWire) -> Result<Self, Self::Error> {
        Self::new(
            wire.name,
            wire.kind,
            wire.executor,
            wire.roles,
            wire.concerns,
        )
    }
}

impl<'de> Deserialize<'de> for OracleStrategyRegistrationV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleStrategyRegistrationWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Canonical strategy registry. Registrations grant proposal capability, never admission authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleStrategyCatalogV1 {
    schema_version: u16,
    strategies: Vec<OracleStrategyRegistrationV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleStrategyCatalogWire {
    schema_version: u16,
    strategies: Vec<OracleStrategyRegistrationV1>,
}

impl OracleStrategyCatalogV1 {
    pub fn new(
        strategies: Vec<OracleStrategyRegistrationV1>,
    ) -> Result<Self, OracleFrameworkError> {
        if strategies.is_empty() {
            return Err(OracleFrameworkError::Empty("strategy catalog"));
        }
        if strategies
            .windows(2)
            .any(|pair| pair[0].name >= pair[1].name)
        {
            return Err(OracleFrameworkError::NonCanonical("strategy catalog"));
        }
        Ok(Self {
            schema_version: SCHEMA_V1,
            strategies,
        })
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleStrategyCatalogArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    #[must_use]
    pub fn strategies(&self) -> &[OracleStrategyRegistrationV1] {
        &self.strategies
    }

    fn eligible(&self, item: &OracleWorkItemV1) -> Vec<OracleStrategyName> {
        self.strategies
            .iter()
            .filter(|strategy| strategy.supports(item))
            .map(|strategy| strategy.name.clone())
            .collect()
    }

    fn resolve(
        &self,
        item: &OracleWorkItemV1,
        strategy: &OracleStrategyName,
    ) -> Result<&OracleStrategyRegistrationV1, OracleFrameworkError> {
        self.strategies
            .iter()
            .find(|registration| registration.name == *strategy && registration.supports(item))
            .ok_or(OracleFrameworkError::IneligibleStrategy)
    }
}

impl TryFrom<OracleStrategyCatalogWire> for OracleStrategyCatalogV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OracleStrategyCatalogWire) -> Result<Self, Self::Error> {
        require_v1(wire.schema_version)?;
        let value = Self::new(wire.strategies)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleStrategyCatalogV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleStrategyCatalogWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OracleStrategyRunLimit(u32);

impl OracleStrategyRunLimit {
    pub const fn new(value: u32) -> Result<Self, OracleFrameworkError> {
        if value == 0 {
            Err(OracleFrameworkError::NonPositive("strategy run limit"))
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for OracleStrategyRunLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OracleExperimentLimit(u32);

impl OracleExperimentLimit {
    pub const fn new(value: u32) -> Result<Self, OracleFrameworkError> {
        if value == 0 {
            Err(OracleFrameworkError::NonPositive("experiment limit"))
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for OracleExperimentLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Monotonic immutable-snapshot revision of one exploration ledger.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OracleExplorationRevision(u32);

impl OracleExplorationRevision {
    pub const fn new(value: u32) -> Result<Self, OracleFrameworkError> {
        if value == 0 {
            Err(OracleFrameworkError::NonPositive("exploration revision"))
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    fn next(self) -> Result<Self, OracleFrameworkError> {
        self.0
            .checked_add(1)
            .ok_or(OracleFrameworkError::RevisionOverflow)
            .and_then(Self::new)
    }
}

impl<'de> Deserialize<'de> for OracleExplorationRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleExplorationBudgetV1 {
    pub strategy_runs: OracleStrategyRunLimit,
    pub experiments: OracleExperimentLimit,
}

/// Exact code, documentation, knowledge, tools and authority available to exploration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleWorkspaceV1 {
    schema_version: u16,
    task_id: TaskId,
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    sir_input: ContentId<IntentRecoveryInputArtifact>,
    sir_task_bundle: ContentId<SirTaskBundleArtifact>,
    source: ContentId<OracleSourceSnapshotArtifact>,
    documentation: ContentId<OracleDocumentationSnapshotArtifact>,
    build_and_tests: ContentId<OracleBuildTestSnapshotArtifact>,
    knowledge: ContentId<OracleKnowledgeSnapshotArtifact>,
    research_tools: ContentId<OracleResearchToolCatalogArtifact>,
    experiment_tools: ContentId<OracleExperimentToolCatalogArtifact>,
    capability_grant: ContentId<OracleExplorationCapabilityGrantArtifact>,
    coverage_policy: ContentId<OracleCoveragePolicyArtifact>,
    strategy_catalog: ContentId<OracleStrategyCatalogArtifact>,
    budget: OracleExplorationBudgetV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleWorkspaceWire {
    schema_version: u16,
    task_id: TaskId,
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    sir_input: ContentId<IntentRecoveryInputArtifact>,
    sir_task_bundle: ContentId<SirTaskBundleArtifact>,
    source: ContentId<OracleSourceSnapshotArtifact>,
    documentation: ContentId<OracleDocumentationSnapshotArtifact>,
    build_and_tests: ContentId<OracleBuildTestSnapshotArtifact>,
    knowledge: ContentId<OracleKnowledgeSnapshotArtifact>,
    research_tools: ContentId<OracleResearchToolCatalogArtifact>,
    experiment_tools: ContentId<OracleExperimentToolCatalogArtifact>,
    capability_grant: ContentId<OracleExplorationCapabilityGrantArtifact>,
    coverage_policy: ContentId<OracleCoveragePolicyArtifact>,
    strategy_catalog: ContentId<OracleStrategyCatalogArtifact>,
    budget: OracleExplorationBudgetV1,
}

/// Explicit constructor input keeps all authority-bearing workspace edges visible.
pub struct OracleWorkspaceInput {
    pub task_id: TaskId,
    pub admitted_intent: ContentId<MigrationIntentContractArtifact>,
    pub sir_input: ContentId<IntentRecoveryInputArtifact>,
    pub sir_task_bundle: ContentId<SirTaskBundleArtifact>,
    pub source: ContentId<OracleSourceSnapshotArtifact>,
    pub documentation: ContentId<OracleDocumentationSnapshotArtifact>,
    pub build_and_tests: ContentId<OracleBuildTestSnapshotArtifact>,
    pub knowledge: ContentId<OracleKnowledgeSnapshotArtifact>,
    pub research_tools: ContentId<OracleResearchToolCatalogArtifact>,
    pub experiment_tools: ContentId<OracleExperimentToolCatalogArtifact>,
    pub capability_grant: ContentId<OracleExplorationCapabilityGrantArtifact>,
    pub coverage_policy: ContentId<OracleCoveragePolicyArtifact>,
    pub strategy_catalog: ContentId<OracleStrategyCatalogArtifact>,
    pub budget: OracleExplorationBudgetV1,
}

impl TryFrom<OracleWorkspaceWire> for OracleWorkspaceV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OracleWorkspaceWire) -> Result<Self, Self::Error> {
        require_v1(wire.schema_version)?;
        let input = OracleWorkspaceInput {
            task_id: wire.task_id,
            admitted_intent: wire.admitted_intent,
            sir_input: wire.sir_input,
            sir_task_bundle: wire.sir_task_bundle,
            source: wire.source,
            documentation: wire.documentation,
            build_and_tests: wire.build_and_tests,
            knowledge: wire.knowledge,
            research_tools: wire.research_tools,
            experiment_tools: wire.experiment_tools,
            capability_grant: wire.capability_grant,
            coverage_policy: wire.coverage_policy,
            strategy_catalog: wire.strategy_catalog,
            budget: wire.budget,
        };
        Ok(Self::new(&input))
    }
}

impl<'de> Deserialize<'de> for OracleWorkspaceV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleWorkspaceWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl OracleWorkspaceV1 {
    #[must_use]
    pub fn new(input: &OracleWorkspaceInput) -> Self {
        Self {
            schema_version: SCHEMA_V1,
            task_id: input.task_id,
            admitted_intent: input.admitted_intent,
            sir_input: input.sir_input,
            sir_task_bundle: input.sir_task_bundle,
            source: input.source,
            documentation: input.documentation,
            build_and_tests: input.build_and_tests,
            knowledge: input.knowledge,
            research_tools: input.research_tools,
            experiment_tools: input.experiment_tools,
            capability_grant: input.capability_grant,
            coverage_policy: input.coverage_policy,
            strategy_catalog: input.strategy_catalog,
            budget: input.budget,
        }
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }
    #[must_use]
    pub const fn admitted_intent(&self) -> ContentId<MigrationIntentContractArtifact> {
        self.admitted_intent
    }
    #[must_use]
    pub const fn sir_input(&self) -> ContentId<IntentRecoveryInputArtifact> {
        self.sir_input
    }
    #[must_use]
    pub const fn sir_task_bundle(&self) -> ContentId<SirTaskBundleArtifact> {
        self.sir_task_bundle
    }
    #[must_use]
    pub const fn source(&self) -> ContentId<OracleSourceSnapshotArtifact> {
        self.source
    }
    #[must_use]
    pub const fn documentation(&self) -> ContentId<OracleDocumentationSnapshotArtifact> {
        self.documentation
    }
    #[must_use]
    pub const fn build_and_tests(&self) -> ContentId<OracleBuildTestSnapshotArtifact> {
        self.build_and_tests
    }
    #[must_use]
    pub const fn knowledge(&self) -> ContentId<OracleKnowledgeSnapshotArtifact> {
        self.knowledge
    }
    #[must_use]
    pub const fn research_tools(&self) -> ContentId<OracleResearchToolCatalogArtifact> {
        self.research_tools
    }
    #[must_use]
    pub const fn experiment_tools(&self) -> ContentId<OracleExperimentToolCatalogArtifact> {
        self.experiment_tools
    }
    #[must_use]
    pub const fn capability_grant(&self) -> ContentId<OracleExplorationCapabilityGrantArtifact> {
        self.capability_grant
    }
    #[must_use]
    pub const fn coverage_policy(&self) -> ContentId<OracleCoveragePolicyArtifact> {
        self.coverage_policy
    }
    #[must_use]
    pub const fn strategy_catalog(&self) -> ContentId<OracleStrategyCatalogArtifact> {
        self.strategy_catalog
    }
    #[must_use]
    pub const fn budget(&self) -> OracleExplorationBudgetV1 {
        self.budget
    }
    pub fn identity(&self) -> Result<ContentId<OracleWorkspaceArtifact>, OracleFrameworkError> {
        derive_id(self)
    }
}

/// One indivisible claim × concern × logical-role obligation.
/// A claim identity cannot be substituted where a fully scoped work-item identity is required.
///
/// ```compile_fail
/// use cairn_migration::{OracleClaimArtifact, OracleWorkItemArtifact};
/// use cairn_protocol::ContentId;
/// fn require_work_item(_: ContentId<OracleWorkItemArtifact>) {}
/// fn wrong(claim: ContentId<OracleClaimArtifact>) { require_work_item(claim); }
/// ```
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct OracleWorkItemV1 {
    claim: ContentId<OracleClaimArtifact>,
    plane: OraclePlaneV1,
    concern: OracleConcernV1,
    role: OracleStrategyRoleV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleWorkItemWire {
    claim: ContentId<OracleClaimArtifact>,
    plane: OraclePlaneV1,
    concern: OracleConcernV1,
    role: OracleStrategyRoleV1,
}

impl Ord for OracleWorkItemV1 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.claim
            .to_wire()
            .cmp(&other.claim.to_wire())
            .then_with(|| self.plane.cmp(&other.plane))
            .then_with(|| self.concern.cmp(&other.concern))
            .then_with(|| self.role.cmp(&other.role))
    }
}

impl TryFrom<OracleWorkItemWire> for OracleWorkItemV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OracleWorkItemWire) -> Result<Self, Self::Error> {
        if wire.plane != wire.concern.plane() {
            return Err(OracleFrameworkError::WorkItemPlaneMismatch);
        }
        Ok(Self::new(wire.claim, wire.concern, wire.role))
    }
}

impl<'de> Deserialize<'de> for OracleWorkItemV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleWorkItemWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl PartialOrd for OracleWorkItemV1 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl OracleWorkItemV1 {
    fn new(
        claim: ContentId<OracleClaimArtifact>,
        concern: OracleConcernV1,
        role: OracleStrategyRoleV1,
    ) -> Self {
        Self {
            claim,
            plane: concern.plane(),
            concern,
            role,
        }
    }

    #[must_use]
    pub const fn claim(&self) -> ContentId<OracleClaimArtifact> {
        self.claim
    }
    #[must_use]
    pub const fn plane(&self) -> OraclePlaneV1 {
        self.plane
    }
    #[must_use]
    pub const fn concern(&self) -> OracleConcernV1 {
        self.concern
    }
    #[must_use]
    pub const fn role(&self) -> OracleStrategyRoleV1 {
        self.role
    }
    pub fn identity(&self) -> Result<ContentId<OracleWorkItemArtifact>, OracleFrameworkError> {
        derive_id(self)
    }
}

/// Deterministically expands every claim across every policy concern and required logical role.
pub fn derive_oracle_work_items(
    claims: &[ContentId<OracleClaimArtifact>],
    policy: &OracleCoveragePolicyV1,
) -> Result<Vec<OracleWorkItemV1>, OracleFrameworkError> {
    validate_content_ids(claims, "oracle claims")?;
    let mut items = Vec::new();
    for claim in claims {
        for concern in policy.concerns() {
            items.push(OracleWorkItemV1::new(
                *claim,
                *concern,
                OracleStrategyRoleV1::Synthesis,
            ));
            if policy.adversarial() == OracleAdversarialPolicyV1::RequiredForEveryConcern {
                items.push(OracleWorkItemV1::new(
                    *claim,
                    *concern,
                    OracleStrategyRoleV1::Adversarial,
                ));
            }
        }
    }
    items.sort();
    Ok(items)
}

/// One exact strategy execution authority for one indivisible work item.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleStrategyRunV1 {
    schema_version: u16,
    workspace: ContentId<OracleWorkspaceArtifact>,
    item: ContentId<OracleWorkItemArtifact>,
    strategy: OracleStrategyName,
    executor: OracleStrategyExecutorV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleStrategyRunWire {
    schema_version: u16,
    workspace: ContentId<OracleWorkspaceArtifact>,
    item: ContentId<OracleWorkItemArtifact>,
    strategy: OracleStrategyName,
    executor: OracleStrategyExecutorV1,
}

impl OracleStrategyRunV1 {
    pub fn new(
        workspace: ContentId<OracleWorkspaceArtifact>,
        item: &OracleWorkItemV1,
        strategy: OracleStrategyName,
        catalog: &OracleStrategyCatalogV1,
    ) -> Result<Self, OracleFrameworkError> {
        let executor = catalog.resolve(item, &strategy)?.executor.clone();
        Ok(Self {
            schema_version: SCHEMA_V1,
            workspace,
            item: item.identity()?,
            strategy,
            executor,
        })
    }

    #[must_use]
    pub const fn workspace(&self) -> ContentId<OracleWorkspaceArtifact> {
        self.workspace
    }
    #[must_use]
    pub const fn item(&self) -> ContentId<OracleWorkItemArtifact> {
        self.item
    }
    #[must_use]
    pub const fn strategy(&self) -> &OracleStrategyName {
        &self.strategy
    }
    #[must_use]
    pub const fn executor(&self) -> &OracleStrategyExecutorV1 {
        &self.executor
    }
    pub fn identity(&self) -> Result<ContentId<OracleStrategyRunArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    fn validate_against(
        &self,
        workspace: ContentId<OracleWorkspaceArtifact>,
        item: &OracleWorkItemV1,
        catalog: &OracleStrategyCatalogV1,
    ) -> Result<(), OracleFrameworkError> {
        let expected = Self::new(workspace, item, self.strategy.clone(), catalog)?;
        if *self != expected {
            return Err(OracleFrameworkError::StrategyRunBindingMismatch);
        }
        Ok(())
    }
}

impl TryFrom<OracleStrategyRunWire> for OracleStrategyRunV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OracleStrategyRunWire) -> Result<Self, Self::Error> {
        require_v1(wire.schema_version)?;
        Ok(Self {
            schema_version: SCHEMA_V1,
            workspace: wire.workspace,
            item: wire.item,
            strategy: wire.strategy,
            executor: wire.executor,
        })
    }
}

impl<'de> Deserialize<'de> for OracleStrategyRunV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleStrategyRunWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Exact external experiment proposed by one active strategy run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleExperimentRequestV1 {
    schema_version: u16,
    item: ContentId<OracleWorkItemArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
    tools: ContentId<OracleExperimentToolCatalogArtifact>,
    operation: OracleExperimentOperationName,
    arguments: ContentId<OracleExperimentArgumentsArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleExperimentRequestWire {
    schema_version: u16,
    item: ContentId<OracleWorkItemArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
    tools: ContentId<OracleExperimentToolCatalogArtifact>,
    operation: OracleExperimentOperationName,
    arguments: ContentId<OracleExperimentArgumentsArtifact>,
}

impl OracleExperimentRequestV1 {
    #[must_use]
    pub fn new(
        item: ContentId<OracleWorkItemArtifact>,
        run: ContentId<OracleStrategyRunArtifact>,
        tools: ContentId<OracleExperimentToolCatalogArtifact>,
        operation: OracleExperimentOperationName,
        arguments: ContentId<OracleExperimentArgumentsArtifact>,
    ) -> Self {
        Self {
            schema_version: SCHEMA_V1,
            item,
            run,
            tools,
            operation,
            arguments,
        }
    }

    #[must_use]
    pub const fn item(&self) -> ContentId<OracleWorkItemArtifact> {
        self.item
    }
    #[must_use]
    pub const fn run(&self) -> ContentId<OracleStrategyRunArtifact> {
        self.run
    }
    #[must_use]
    pub const fn tools(&self) -> ContentId<OracleExperimentToolCatalogArtifact> {
        self.tools
    }
    #[must_use]
    pub const fn arguments(&self) -> ContentId<OracleExperimentArgumentsArtifact> {
        self.arguments
    }
    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleExperimentRequestArtifact>, OracleFrameworkError> {
        derive_id(self)
    }
}

impl TryFrom<OracleExperimentRequestWire> for OracleExperimentRequestV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OracleExperimentRequestWire) -> Result<Self, Self::Error> {
        require_v1(wire.schema_version)?;
        Ok(Self::new(
            wire.item,
            wire.run,
            wire.tools,
            wire.operation,
            wire.arguments,
        ))
    }
}

impl<'de> Deserialize<'de> for OracleExperimentRequestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleExperimentRequestWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Controller-qualified binding from an Oracle experiment to generic Worker evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TrustedOracleWorkerReceiptV1 {
    schema_version: u16,
    request: ContentId<OracleExperimentRequestArtifact>,
    job_contract: ContentId<JobContractArtifact>,
    execution_receipt: ContentId<ExecutionReceiptArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustedOracleWorkerReceiptWire {
    schema_version: u16,
    request: ContentId<OracleExperimentRequestArtifact>,
    job_contract: ContentId<JobContractArtifact>,
    execution_receipt: ContentId<ExecutionReceiptArtifact>,
}

impl TrustedOracleWorkerReceiptV1 {
    #[must_use]
    pub fn new(
        request: ContentId<OracleExperimentRequestArtifact>,
        job_contract: ContentId<JobContractArtifact>,
        execution_receipt: ContentId<ExecutionReceiptArtifact>,
    ) -> Self {
        Self {
            schema_version: SCHEMA_V1,
            request,
            job_contract,
            execution_receipt,
        }
    }
    #[must_use]
    pub const fn request(&self) -> ContentId<OracleExperimentRequestArtifact> {
        self.request
    }
    pub fn identity(
        &self,
    ) -> Result<ContentId<TrustedOracleWorkerReceiptArtifact>, OracleFrameworkError> {
        derive_id(self)
    }
}

impl TryFrom<TrustedOracleWorkerReceiptWire> for TrustedOracleWorkerReceiptV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: TrustedOracleWorkerReceiptWire) -> Result<Self, Self::Error> {
        require_v1(wire.schema_version)?;
        Ok(Self::new(
            wire.request,
            wire.job_contract,
            wire.execution_receipt,
        ))
    }
}

impl<'de> Deserialize<'de> for TrustedOracleWorkerReceiptV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        TrustedOracleWorkerReceiptWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Evidence explaining why one exact cell remains unknown after a strategy run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleUnknownEvidenceV1 {
    schema_version: u16,
    item: ContentId<OracleWorkItemArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
    reason: OracleUnknownReason,
    observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleUnknownEvidenceWire {
    schema_version: u16,
    item: ContentId<OracleWorkItemArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
    reason: OracleUnknownReason,
    observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
}

impl OracleUnknownEvidenceV1 {
    pub fn new(
        item: ContentId<OracleWorkItemArtifact>,
        run: ContentId<OracleStrategyRunArtifact>,
        reason: OracleUnknownReason,
        mut observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
    ) -> Result<Self, OracleFrameworkError> {
        observations.sort_by_key(ContentId::to_wire);
        validate_content_id_order(&observations, "unknown observations")?;
        Ok(Self {
            schema_version: SCHEMA_V1,
            item,
            run,
            reason,
            observations,
        })
    }

    #[must_use]
    pub const fn item(&self) -> ContentId<OracleWorkItemArtifact> {
        self.item
    }
    #[must_use]
    pub const fn run(&self) -> ContentId<OracleStrategyRunArtifact> {
        self.run
    }
    #[must_use]
    pub fn observations(&self) -> &[ContentId<OracleExplorationObservationArtifact>] {
        &self.observations
    }
    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleUnknownEvidenceArtifact>, OracleFrameworkError> {
        derive_id(self)
    }
}

impl TryFrom<OracleUnknownEvidenceWire> for OracleUnknownEvidenceV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OracleUnknownEvidenceWire) -> Result<Self, Self::Error> {
        require_v1(wire.schema_version)?;
        Self::new(wire.item, wire.run, wire.reason, wire.observations)
    }
}

impl<'de> Deserialize<'de> for OracleUnknownEvidenceV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleUnknownEvidenceWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Provenance class for an observation. Origin records facts but grants no admission authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "origin", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OracleObservationProvenanceV1 {
    Deterministic {
        implementation: ContentId<OracleStrategyImplementationArtifact>,
    },
    ControllerResearch {
        exchange: ContentId<OracleResearchExchangeArtifact>,
    },
    WorkerExperiment {
        request: ContentId<OracleExperimentRequestArtifact>,
        receipt: ContentId<TrustedOracleWorkerReceiptArtifact>,
    },
    WorkflowTool {
        observation: ContentId<WorkflowToolControllerObservationArtifact>,
    },
}

/// Exact model-visible effect payload archived under the Oracle observation domain.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleObservationPayloadV1 {
    schema_version: u16,
    source: ContentId<WorkflowToolControllerObservationArtifact>,
    value: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleObservationPayloadWire {
    schema_version: u16,
    source: ContentId<WorkflowToolControllerObservationArtifact>,
    value: serde_json::Value,
}

impl OracleObservationPayloadV1 {
    #[must_use]
    pub fn new(
        source: ContentId<WorkflowToolControllerObservationArtifact>,
        value: serde_json::Value,
    ) -> Self {
        Self {
            schema_version: SCHEMA_V1,
            source,
            value,
        }
    }

    #[must_use]
    pub const fn source(&self) -> ContentId<WorkflowToolControllerObservationArtifact> {
        self.source
    }

    #[must_use]
    pub const fn value(&self) -> &serde_json::Value {
        &self.value
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleObservationPayloadArtifact>, OracleFrameworkError> {
        derive_id(self)
    }
}

impl TryFrom<OracleObservationPayloadWire> for OracleObservationPayloadV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleObservationPayloadWire) -> Result<Self, Self::Error> {
        require_v1(wire.schema_version)?;
        Ok(Self::new(wire.source, wire.value))
    }
}

impl<'de> Deserialize<'de> for OracleObservationPayloadV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleObservationPayloadWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Exact provenance-bearing observation projected into one strategy run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleExplorationObservationV1 {
    schema_version: u16,
    item: ContentId<OracleWorkItemArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
    provenance: OracleObservationProvenanceV1,
    payload: ContentId<OracleObservationPayloadArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleExplorationObservationWire {
    schema_version: u16,
    item: ContentId<OracleWorkItemArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
    provenance: OracleObservationProvenanceV1,
    payload: ContentId<OracleObservationPayloadArtifact>,
}

impl OracleExplorationObservationV1 {
    pub fn workflow_tool(
        item: ContentId<OracleWorkItemArtifact>,
        run: ContentId<OracleStrategyRunArtifact>,
        source: ContentId<WorkflowToolControllerObservationArtifact>,
        payload: &OracleObservationPayloadV1,
    ) -> Result<Self, OracleFrameworkError> {
        if payload.source() != source {
            return Err(OracleFrameworkError::ObservationBindingMismatch);
        }
        Ok(Self {
            schema_version: SCHEMA_V1,
            item,
            run,
            provenance: OracleObservationProvenanceV1::WorkflowTool {
                observation: source,
            },
            payload: payload.identity()?,
        })
    }

    pub fn worker_experiment(
        request: &OracleExperimentRequestV1,
        receipt: &TrustedOracleWorkerReceiptV1,
        payload: ContentId<OracleObservationPayloadArtifact>,
    ) -> Result<Self, OracleFrameworkError> {
        let request_id = request.identity()?;
        if receipt.request() != request_id {
            return Err(OracleFrameworkError::ExperimentBindingMismatch);
        }
        Ok(Self {
            schema_version: SCHEMA_V1,
            item: request.item(),
            run: request.run(),
            provenance: OracleObservationProvenanceV1::WorkerExperiment {
                request: request_id,
                receipt: receipt.identity()?,
            },
            payload,
        })
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleExplorationObservationArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    #[must_use]
    pub const fn item(&self) -> ContentId<OracleWorkItemArtifact> {
        self.item
    }

    #[must_use]
    pub const fn run(&self) -> ContentId<OracleStrategyRunArtifact> {
        self.run
    }

    fn validates_worker_binding(
        &self,
        request: &OracleExperimentRequestV1,
        receipt: &TrustedOracleWorkerReceiptV1,
    ) -> Result<bool, OracleFrameworkError> {
        Ok(self.item == request.item()
            && self.run == request.run()
            && matches!(
                self.provenance,
                OracleObservationProvenanceV1::WorkerExperiment {
                    request: request_id,
                    receipt: receipt_id,
                } if request_id == request.identity()? && receipt_id == receipt.identity()?
            ))
    }
}

impl TryFrom<OracleExplorationObservationWire> for OracleExplorationObservationV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OracleExplorationObservationWire) -> Result<Self, Self::Error> {
        require_v1(wire.schema_version)?;
        Ok(Self {
            schema_version: SCHEMA_V1,
            item: wire.item,
            run: wire.run,
            provenance: wire.provenance,
            payload: wire.payload,
        })
    }
}

impl<'de> Deserialize<'de> for OracleExplorationObservationV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleExplorationObservationWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Typed Oracle material contributed by one strategy to one coverage cell.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "artifact", rename_all = "kebab-case")]
pub enum OraclePortfolioElementKindV1 {
    DomainRefinement(ContentId<DomainRefinementArtifact>),
    CorpusCase(ContentId<CorpusCaseArtifact>),
    Reference(ContentId<ReferenceArtifact>),
    PropertyRelation(ContentId<PropertyRelationArtifact>),
    SourceAdmissionPlan(ContentId<SourceAdmissionPlanArtifact>),
    ValidFamilyPlan(ContentId<ValidFamilyPlanArtifact>),
    ObservationPlan(ContentId<ObservationPlanArtifact>),
    Comparator(ContentId<OracleComparatorProposalArtifact>),
    ExecutionSafety(ContentId<OracleExecutionSafetyProposalArtifact>),
    CoverageGap(ContentId<OracleCoverageGapArtifact>),
}

/// One typed proposal element with exact cell, run and observation lineage.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OraclePortfolioElementV1 {
    schema_version: u16,
    item: ContentId<OracleWorkItemArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
    kind: OraclePortfolioElementKindV1,
    observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OraclePortfolioElementWire {
    schema_version: u16,
    item: ContentId<OracleWorkItemArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
    kind: OraclePortfolioElementKindV1,
    observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
}

impl OraclePortfolioElementV1 {
    pub fn new(
        item: ContentId<OracleWorkItemArtifact>,
        run: ContentId<OracleStrategyRunArtifact>,
        kind: OraclePortfolioElementKindV1,
        mut observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
    ) -> Result<Self, OracleFrameworkError> {
        observations.sort_by_key(ContentId::to_wire);
        validate_content_id_order(&observations, "portfolio element observations")?;
        Ok(Self {
            schema_version: SCHEMA_V1,
            item,
            run,
            kind,
            observations,
        })
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OraclePortfolioElementArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    #[must_use]
    pub const fn item(&self) -> ContentId<OracleWorkItemArtifact> {
        self.item
    }

    #[must_use]
    pub const fn run(&self) -> ContentId<OracleStrategyRunArtifact> {
        self.run
    }

    #[must_use]
    pub const fn kind(&self) -> &OraclePortfolioElementKindV1 {
        &self.kind
    }

    #[must_use]
    pub fn observations(&self) -> &[ContentId<OracleExplorationObservationArtifact>] {
        &self.observations
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        validate_content_id_order(&self.observations, "portfolio element observations")
    }
}

impl TryFrom<OraclePortfolioElementWire> for OraclePortfolioElementV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OraclePortfolioElementWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            item: wire.item,
            run: wire.run,
            kind: wire.kind,
            observations: wire.observations,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OraclePortfolioElementV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OraclePortfolioElementWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// One strict terminal submission from a single cell-scoped strategy run.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OracleStrategySubmissionOutcomeV1 {
    Contribute {
        elements: Vec<OraclePortfolioElementV1>,
    },
    RequestExperiment {
        request: OracleExperimentRequestV1,
    },
    PreserveUnknown {
        evidence: Vec<OracleUnknownEvidenceV1>,
    },
}

/// Atomic strategy publication bound to one exact run and work item.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleStrategySubmissionV1 {
    schema_version: u16,
    run: ContentId<OracleStrategyRunArtifact>,
    item: ContentId<OracleWorkItemArtifact>,
    result: OracleStrategySubmissionOutcomeV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleStrategySubmissionWire {
    schema_version: u16,
    run: ContentId<OracleStrategyRunArtifact>,
    item: ContentId<OracleWorkItemArtifact>,
    result: OracleStrategySubmissionOutcomeV1,
}

impl OracleStrategySubmissionV1 {
    pub fn new(
        run: &OracleStrategyRunV1,
        result: OracleStrategySubmissionOutcomeV1,
    ) -> Result<Self, OracleFrameworkError> {
        let value = Self {
            schema_version: SCHEMA_V1,
            run: run.identity()?,
            item: run.item(),
            result,
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub const fn run(&self) -> ContentId<OracleStrategyRunArtifact> {
        self.run
    }
    #[must_use]
    pub const fn item(&self) -> ContentId<OracleWorkItemArtifact> {
        self.item
    }
    #[must_use]
    pub const fn result(&self) -> &OracleStrategySubmissionOutcomeV1 {
        &self.result
    }
    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleStrategySubmissionArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        match &self.result {
            OracleStrategySubmissionOutcomeV1::Contribute { elements } => {
                let ids = elements
                    .iter()
                    .map(|element| {
                        element.validate()?;
                        if element.item != self.item || element.run != self.run {
                            return Err(OracleFrameworkError::PortfolioElementBindingMismatch);
                        }
                        element.identity()
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                validate_content_ids(&ids, "strategy contribution")
            }
            OracleStrategySubmissionOutcomeV1::RequestExperiment { request } => {
                if request.item() != self.item || request.run() != self.run {
                    return Err(OracleFrameworkError::ExperimentBindingMismatch);
                }
                Ok(())
            }
            OracleStrategySubmissionOutcomeV1::PreserveUnknown { evidence } => {
                let ids = evidence
                    .iter()
                    .map(|value| {
                        if value.item() != self.item || value.run() != self.run {
                            return Err(OracleFrameworkError::StrategyRunBindingMismatch);
                        }
                        value.identity()
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                validate_content_ids(&ids, "strategy unknown evidence")
            }
        }
    }
}

impl TryFrom<OracleStrategySubmissionWire> for OracleStrategySubmissionV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OracleStrategySubmissionWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            run: wire.run,
            item: wire.item,
            result: wire.result,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleStrategySubmissionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleStrategySubmissionWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OracleObligationResolutionV1 {
    Pending,
    Running {
        run: ContentId<OracleStrategyRunArtifact>,
        strategy: OracleStrategyName,
        observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
    },
    NeedsExperiment {
        run: ContentId<OracleStrategyRunArtifact>,
        strategy: OracleStrategyName,
        request: ContentId<OracleExperimentRequestArtifact>,
        observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
    },
    AwaitingExperiment {
        run: ContentId<OracleStrategyRunArtifact>,
        strategy: OracleStrategyName,
        request: ContentId<OracleExperimentRequestArtifact>,
        observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
    },
    Contributed {
        run: ContentId<OracleStrategyRunArtifact>,
        elements: Vec<ContentId<OraclePortfolioElementArtifact>>,
        observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
    },
    CoverageGap {
        run: ContentId<OracleStrategyRunArtifact>,
        elements: Vec<ContentId<OraclePortfolioElementArtifact>>,
        observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
    },
    Unknown {
        evidence: Vec<ContentId<OracleUnknownEvidenceArtifact>>,
    },
    Unsupported {
        evidence: Vec<ContentId<OracleUnknownEvidenceArtifact>>,
    },
    PolicyWaived {
        authority: ContentId<OracleWaiverAuthorityArtifact>,
    },
}

impl OracleObligationResolutionV1 {
    fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Contributed { .. }
                | Self::CoverageGap { .. }
                | Self::Unknown { .. }
                | Self::Unsupported { .. }
                | Self::PolicyWaived { .. }
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OracleObligationEntryV1 {
    item: OracleWorkItemV1,
    resolution: OracleObligationResolutionV1,
}

impl OracleObligationEntryV1 {
    #[must_use]
    pub const fn item(&self) -> &OracleWorkItemV1 {
        &self.item
    }
    #[must_use]
    pub const fn resolution(&self) -> &OracleObligationResolutionV1 {
        &self.resolution
    }
}

/// Immutable-snapshot durable exploration ledger. Every revision retains all work items.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleExplorationLedgerV1 {
    schema_version: u16,
    workspace: ContentId<OracleWorkspaceArtifact>,
    parent: Option<ContentId<OracleExplorationLedgerArtifact>>,
    revision: OracleExplorationRevision,
    entries: Vec<OracleObligationEntryV1>,
    strategy_runs_started: u32,
    experiments_started: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleExplorationLedgerWire {
    schema_version: u16,
    workspace: ContentId<OracleWorkspaceArtifact>,
    parent: Option<ContentId<OracleExplorationLedgerArtifact>>,
    revision: OracleExplorationRevision,
    entries: Vec<OracleObligationEntryV1>,
    strategy_runs_started: u32,
    experiments_started: u32,
}

impl OracleExplorationLedgerV1 {
    pub fn open(
        workspace_id: ContentId<OracleWorkspaceArtifact>,
        work_items: Vec<OracleWorkItemV1>,
        catalog: &OracleStrategyCatalogV1,
    ) -> Result<Self, OracleFrameworkError> {
        validate_strict(&work_items, "oracle work items")?;
        for item in &work_items {
            if catalog.eligible(item).is_empty() {
                return Err(OracleFrameworkError::MissingStrategy {
                    plane: item.plane,
                    concern: item.concern,
                    role: item.role,
                });
            }
        }
        Ok(Self {
            schema_version: SCHEMA_V1,
            workspace: workspace_id,
            parent: None,
            revision: OracleExplorationRevision::new(1)?,
            entries: work_items
                .into_iter()
                .map(|item| OracleObligationEntryV1 {
                    item,
                    resolution: OracleObligationResolutionV1::Pending,
                })
                .collect(),
            strategy_runs_started: 0,
            experiments_started: 0,
        })
    }

    #[must_use]
    pub fn entries(&self) -> &[OracleObligationEntryV1] {
        &self.entries
    }

    #[must_use]
    pub const fn workspace(&self) -> ContentId<OracleWorkspaceArtifact> {
        self.workspace
    }

    #[must_use]
    pub const fn parent(&self) -> Option<ContentId<OracleExplorationLedgerArtifact>> {
        self.parent
    }

    #[must_use]
    pub const fn revision(&self) -> OracleExplorationRevision {
        self.revision
    }

    /// Commits one exact eligible strategy run before its deterministic or model executor starts.
    pub fn start_strategy(
        &self,
        run: &OracleStrategyRunV1,
        catalog: &OracleStrategyCatalogV1,
        budget: OracleExplorationBudgetV1,
    ) -> Result<Self, OracleFrameworkError> {
        if self.strategy_runs_started >= budget.strategy_runs.get() {
            return Err(OracleFrameworkError::StrategyBudgetExhausted);
        }
        let index = self.entry_index(run.item())?;
        if !matches!(
            self.entries[index].resolution,
            OracleObligationResolutionV1::Pending
        ) {
            return Err(OracleFrameworkError::InvalidLedgerTransition);
        }
        run.validate_against(self.workspace, &self.entries[index].item, catalog)?;
        let run_id = run.identity()?;
        let strategy = run.strategy().clone();
        self.revise(|next| {
            next.strategy_runs_started += 1;
            next.entries[index].resolution = OracleObligationResolutionV1::Running {
                run: run_id,
                strategy,
                observations: Vec::new(),
            };
            Ok(())
        })
    }

    /// Projects Controller-produced effect observations into the exact active strategy run.
    pub fn record_strategy_observations(
        &self,
        item: ContentId<OracleWorkItemArtifact>,
        run: ContentId<OracleStrategyRunArtifact>,
        observations: &[OracleExplorationObservationV1],
    ) -> Result<Self, OracleFrameworkError> {
        if observations.is_empty()
            || observations
                .iter()
                .any(|observation| observation.item() != item || observation.run() != run)
        {
            return Err(OracleFrameworkError::ObservationBindingMismatch);
        }
        let mut new_ids = observations
            .iter()
            .map(OracleExplorationObservationV1::identity)
            .collect::<Result<Vec<_>, _>>()?;
        new_ids.sort_by_key(ContentId::to_wire);
        validate_content_ids(&new_ids, "strategy observations")?;
        let index = self.entry_index(item)?;
        let OracleObligationResolutionV1::Running {
            run: active,
            observations: current,
            ..
        } = &self.entries[index].resolution
        else {
            return Err(OracleFrameworkError::InvalidLedgerTransition);
        };
        if *active != run || new_ids.iter().any(|id| current.contains(id)) {
            return Err(OracleFrameworkError::ObservationBindingMismatch);
        }
        self.revise(|next| {
            let OracleObligationResolutionV1::Running { observations, .. } =
                &mut next.entries[index].resolution
            else {
                unreachable!()
            };
            observations.extend(new_ids);
            observations.sort_by_key(ContentId::to_wire);
            Ok(())
        })
    }

    /// Records a strategy experiment proposal without granting effect authority.
    pub fn request_experiment(
        &self,
        request: &OracleExperimentRequestV1,
        workspace: &OracleWorkspaceV1,
    ) -> Result<Self, OracleFrameworkError> {
        if workspace.identity()? != self.workspace
            || request.tools() != workspace.experiment_tools()
        {
            return Err(OracleFrameworkError::ExperimentBindingMismatch);
        }
        let index = self.entry_index(request.item())?;
        let OracleObligationResolutionV1::Running {
            run: active,
            strategy,
            observations,
        } = &self.entries[index].resolution
        else {
            return Err(OracleFrameworkError::InvalidLedgerTransition);
        };
        if *active != request.run() {
            return Err(OracleFrameworkError::StrategyRunBindingMismatch);
        }
        let request_id = request.identity()?;
        let run = request.run();
        let strategy = strategy.clone();
        let observations = observations.clone();
        self.revise(|next| {
            next.entries[index].resolution = OracleObligationResolutionV1::NeedsExperiment {
                run,
                strategy,
                request: request_id,
                observations,
            };
            Ok(())
        })
    }

    /// Commits Controller start authority before the external experiment may execute.
    pub fn authorize_experiment(
        &self,
        request: &OracleExperimentRequestV1,
        budget: OracleExplorationBudgetV1,
    ) -> Result<Self, OracleFrameworkError> {
        if self.experiments_started >= budget.experiments.get() {
            return Err(OracleFrameworkError::ExperimentBudgetExhausted);
        }
        let index = self.entry_index(request.item())?;
        let OracleObligationResolutionV1::NeedsExperiment {
            run,
            strategy,
            request: pending,
            observations,
        } = &self.entries[index].resolution
        else {
            return Err(OracleFrameworkError::InvalidLedgerTransition);
        };
        let request_id = request.identity()?;
        if *pending != request_id || *run != request.run() {
            return Err(OracleFrameworkError::ExperimentBindingMismatch);
        }
        let run = *run;
        let strategy = strategy.clone();
        let observations = observations.clone();
        self.revise(|next| {
            next.experiments_started += 1;
            next.entries[index].resolution = OracleObligationResolutionV1::AwaitingExperiment {
                run,
                strategy,
                request: request_id,
                observations,
            };
            Ok(())
        })
    }

    /// Projects one Controller-validated observation back to the requesting strategy run.
    pub fn record_experiment_observation(
        &self,
        request: &OracleExperimentRequestV1,
        receipt: &TrustedOracleWorkerReceiptV1,
        observation: &OracleExplorationObservationV1,
    ) -> Result<Self, OracleFrameworkError> {
        let index = self.entry_index(request.item())?;
        let OracleObligationResolutionV1::AwaitingExperiment {
            run,
            strategy,
            request: active,
            observations,
        } = &self.entries[index].resolution
        else {
            return Err(OracleFrameworkError::InvalidLedgerTransition);
        };
        let request_id = request.identity()?;
        if *active != request_id || *run != request.run() {
            return Err(OracleFrameworkError::ExperimentBindingMismatch);
        }
        let observation_id = observation.identity()?;
        if observation.schema_version != SCHEMA_V1
            || observation.item != request.item()
            || observation.run != *run
            || !observation.validates_worker_binding(request, receipt)?
        {
            return Err(OracleFrameworkError::ObservationBindingMismatch);
        }
        let run = *run;
        let strategy = strategy.clone();
        let mut observations = observations.clone();
        if observations.contains(&observation_id) {
            return Err(OracleFrameworkError::DuplicateObservation);
        }
        observations.push(observation_id);
        observations.sort_by_key(ContentId::to_wire);
        self.revise(|next| {
            next.entries[index].resolution = OracleObligationResolutionV1::Running {
                run,
                strategy,
                observations,
            };
            Ok(())
        })
    }

    /// Freezes proposal elements from a running strategy without granting admission authority.
    pub fn record_contribution(
        &self,
        item: ContentId<OracleWorkItemArtifact>,
        run: ContentId<OracleStrategyRunArtifact>,
        elements: &[OraclePortfolioElementV1],
    ) -> Result<Self, OracleFrameworkError> {
        if elements
            .iter()
            .any(|element| element.item != item || element.run != run)
        {
            return Err(OracleFrameworkError::PortfolioElementBindingMismatch);
        }
        let gaps = elements
            .iter()
            .filter(|element| matches!(element.kind, OraclePortfolioElementKindV1::CoverageGap(_)))
            .count();
        if gaps != 0 && gaps != elements.len() {
            return Err(OracleFrameworkError::MixedCoverageGapContribution);
        }
        let mut element_ids = elements
            .iter()
            .map(OraclePortfolioElementV1::identity)
            .collect::<Result<Vec<_>, _>>()?;
        element_ids.sort_by_key(ContentId::to_wire);
        validate_content_ids(&element_ids, "portfolio contribution")?;
        let index = self.entry_index(item)?;
        let OracleObligationResolutionV1::Running {
            run: active,
            observations,
            ..
        } = &self.entries[index].resolution
        else {
            return Err(OracleFrameworkError::InvalidLedgerTransition);
        };
        if *active != run {
            return Err(OracleFrameworkError::StrategyRunBindingMismatch);
        }
        if elements.iter().any(|element| {
            element
                .observations
                .iter()
                .any(|observation| !observations.contains(observation))
        }) {
            return Err(OracleFrameworkError::ObservationBindingMismatch);
        }
        let observations = observations.clone();
        self.revise(|next| {
            next.entries[index].resolution = if gaps == elements.len() {
                OracleObligationResolutionV1::CoverageGap {
                    run,
                    elements: element_ids,
                    observations,
                }
            } else {
                OracleObligationResolutionV1::Contributed {
                    run,
                    elements: element_ids,
                    observations,
                }
            };
            Ok(())
        })
    }

    /// Preserves an evidenced unknown rather than allowing a strategy to silently skip a cell.
    pub fn record_unknown(
        &self,
        item: ContentId<OracleWorkItemArtifact>,
        run: ContentId<OracleStrategyRunArtifact>,
        evidence: &[OracleUnknownEvidenceV1],
    ) -> Result<Self, OracleFrameworkError> {
        if evidence
            .iter()
            .any(|value| value.item() != item || value.run() != run)
        {
            return Err(OracleFrameworkError::StrategyRunBindingMismatch);
        }
        let mut evidence_ids = evidence
            .iter()
            .map(OracleUnknownEvidenceV1::identity)
            .collect::<Result<Vec<_>, _>>()?;
        evidence_ids.sort_by_key(ContentId::to_wire);
        validate_content_ids(&evidence_ids, "unknown evidence")?;
        let index = self.entry_index(item)?;
        let OracleObligationResolutionV1::Running {
            run: active,
            ref observations,
            ..
        } = self.entries[index].resolution
        else {
            return Err(OracleFrameworkError::InvalidLedgerTransition);
        };
        if active != run {
            return Err(OracleFrameworkError::StrategyRunBindingMismatch);
        }
        if evidence.iter().any(|value| {
            value
                .observations
                .iter()
                .any(|observation| !observations.contains(observation))
        }) {
            return Err(OracleFrameworkError::ObservationBindingMismatch);
        }
        self.revise(|next| {
            next.entries[index].resolution = OracleObligationResolutionV1::Unknown {
                evidence: evidence_ids,
            };
            Ok(())
        })
    }

    /// Applies one atomically validated cell submission to the active run.
    pub fn apply_strategy_submission(
        &self,
        run: &OracleStrategyRunV1,
        submission: &OracleStrategySubmissionV1,
        workspace: &OracleWorkspaceV1,
    ) -> Result<Self, OracleFrameworkError> {
        submission.validate()?;
        if run.identity()? != submission.run()
            || run.item() != submission.item()
            || run.workspace() != self.workspace
            || workspace.identity()? != self.workspace
        {
            return Err(OracleFrameworkError::StrategyRunBindingMismatch);
        }
        match submission.result() {
            OracleStrategySubmissionOutcomeV1::Contribute { elements } => {
                self.record_contribution(run.item(), submission.run(), elements)
            }
            OracleStrategySubmissionOutcomeV1::RequestExperiment { request } => {
                self.request_experiment(request, workspace)
            }
            OracleStrategySubmissionOutcomeV1::PreserveUnknown { evidence } => {
                self.record_unknown(run.item(), submission.run(), evidence)
            }
        }
    }

    pub fn next_action(
        &self,
        catalog: &OracleStrategyCatalogV1,
        budget: OracleExplorationBudgetV1,
    ) -> Result<OracleExplorationNextActionV1, OracleFrameworkError> {
        if let Some(entry) = self.entries.iter().find(|entry| {
            matches!(
                entry.resolution,
                OracleObligationResolutionV1::NeedsExperiment { .. }
            )
        }) {
            let OracleObligationResolutionV1::NeedsExperiment { request, .. } = entry.resolution
            else {
                unreachable!()
            };
            if self.experiments_started >= budget.experiments.get() {
                return Ok(OracleExplorationNextActionV1::BudgetExhausted);
            }
            return Ok(OracleExplorationNextActionV1::AuthorizeExperiment {
                item: entry.item.clone(),
                request,
            });
        }
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| matches!(entry.resolution, OracleObligationResolutionV1::Pending))
        {
            if self.strategy_runs_started >= budget.strategy_runs.get() {
                return Ok(OracleExplorationNextActionV1::BudgetExhausted);
            }
            let strategies = catalog.eligible(&entry.item);
            if strategies.is_empty() {
                return Err(OracleFrameworkError::MissingStrategy {
                    plane: entry.item.plane,
                    concern: entry.item.concern,
                    role: entry.item.role,
                });
            }
            return Ok(OracleExplorationNextActionV1::RunStrategy {
                item: entry.item.clone(),
                eligible_strategies: strategies,
            });
        }
        if self
            .entries
            .iter()
            .any(|entry| !entry.resolution.is_terminal())
        {
            return Ok(OracleExplorationNextActionV1::AwaitObservation);
        }
        Ok(OracleExplorationNextActionV1::FreezePortfolio)
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleExplorationLedgerArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        if (self.revision.get() == 1 && self.parent.is_some())
            || (self.revision.get() > 1 && self.parent.is_none())
        {
            return Err(OracleFrameworkError::RevisionLineageMismatch);
        }
        let items: Vec<_> = self
            .entries
            .iter()
            .map(|entry| entry.item.clone())
            .collect();
        validate_strict(&items, "oracle ledger work items")?;
        for entry in &self.entries {
            match &entry.resolution {
                OracleObligationResolutionV1::Contributed { elements, .. }
                | OracleObligationResolutionV1::CoverageGap { elements, .. }
                    if elements.is_empty() =>
                {
                    return Err(OracleFrameworkError::Empty("portfolio contribution"));
                }
                OracleObligationResolutionV1::Contributed {
                    elements,
                    observations,
                    ..
                }
                | OracleObligationResolutionV1::CoverageGap {
                    elements,
                    observations,
                    ..
                } => {
                    validate_content_ids(elements, "portfolio elements")?;
                    validate_content_id_order(observations, "exploration observations")?;
                }
                OracleObligationResolutionV1::Unknown { evidence }
                | OracleObligationResolutionV1::Unsupported { evidence } => {
                    validate_content_ids(evidence, "unknown evidence")?;
                }
                OracleObligationResolutionV1::Running { observations, .. }
                | OracleObligationResolutionV1::NeedsExperiment { observations, .. }
                | OracleObligationResolutionV1::AwaitingExperiment { observations, .. } => {
                    validate_content_id_order(observations, "exploration observations")?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn entry_index(
        &self,
        item: ContentId<OracleWorkItemArtifact>,
    ) -> Result<usize, OracleFrameworkError> {
        self.entries
            .iter()
            .position(|entry| entry.item.identity().is_ok_and(|identity| identity == item))
            .ok_or(OracleFrameworkError::UnknownWorkItem)
    }

    fn revise(
        &self,
        mutation: impl FnOnce(&mut Self) -> Result<(), OracleFrameworkError>,
    ) -> Result<Self, OracleFrameworkError> {
        let mut next = self.clone();
        next.parent = Some(self.identity()?);
        next.revision = self.revision.next()?;
        mutation(&mut next)?;
        next.validate()?;
        Ok(next)
    }
}

impl TryFrom<OracleExplorationLedgerWire> for OracleExplorationLedgerV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OracleExplorationLedgerWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            workspace: wire.workspace,
            parent: wire.parent,
            revision: wire.revision,
            entries: wire.entries,
            strategy_runs_started: wire.strategy_runs_started,
            experiments_started: wire.experiments_started,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleExplorationLedgerV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleExplorationLedgerWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OracleExplorationNextActionV1 {
    RunStrategy {
        item: OracleWorkItemV1,
        eligible_strategies: Vec<OracleStrategyName>,
    },
    AuthorizeExperiment {
        item: OracleWorkItemV1,
        request: ContentId<OracleExperimentRequestArtifact>,
    },
    AwaitObservation,
    FreezePortfolio,
    BudgetExhausted,
}

/// Architectural decision selected from one recovered Oracle Exploration state.
pub enum OracleExplorationDirectiveV1<StrategyAction, ExperimentAction, Waiting> {
    RunStrategy(StrategyAction),
    AuthorizeControllerExperiment(ExperimentAction),
    AwaitObservation(Waiting),
    FreezePortfolio,
    BudgetExhausted,
}

/// A complete proposal or an explicit non-successful stopping boundary.
pub enum OracleExplorationRunOutcomeV1<Portfolio, Waiting> {
    Portfolio(Portfolio),
    AwaitingObservation(Waiting),
    BudgetExhausted,
}

/// Ports for the readable, strategy-independent Oracle Exploration business loop.
pub trait OracleExplorationStages: Send {
    type Error: Send;
    type AdmittedIntent: Send + Sync;
    type Workspace: Send + Sync;
    type Claims: Send + Sync;
    type WorkItems: Send + Sync;
    type Exploration: Send;
    type StrategyAction: Send;
    type ExperimentAction: Send;
    type Waiting: Send;
    type Portfolio: Send;

    fn freeze_oracle_workspace(
        &mut self,
        intent: &Self::AdmittedIntent,
    ) -> impl Future<Output = Result<Self::Workspace, Self::Error>> + Send;
    fn derive_oracle_claims(
        &mut self,
        workspace: &Self::Workspace,
        intent: &Self::AdmittedIntent,
    ) -> Result<Self::Claims, Self::Error>;
    fn derive_oracle_work_items(
        &mut self,
        workspace: &Self::Workspace,
        claims: &Self::Claims,
    ) -> Result<Self::WorkItems, Self::Error>;
    fn open_or_recover_oracle_exploration(
        &mut self,
        workspace: &Self::Workspace,
        work_items: &Self::WorkItems,
    ) -> Result<Self::Exploration, Self::Error>;
    #[allow(
        clippy::type_complexity,
        reason = "the directive keeps three authority-distinct action types"
    )]
    fn select_next_oracle_action(
        &mut self,
        exploration: &Self::Exploration,
    ) -> Result<
        OracleExplorationDirectiveV1<Self::StrategyAction, Self::ExperimentAction, Self::Waiting>,
        Self::Error,
    >;
    fn run_oracle_strategy(
        &mut self,
        workspace: &Self::Workspace,
        exploration: Self::Exploration,
        action: Self::StrategyAction,
    ) -> impl Future<Output = Result<Self::Exploration, Self::Error>> + Send;
    fn authorize_and_run_oracle_experiment(
        &mut self,
        workspace: &Self::Workspace,
        exploration: Self::Exploration,
        action: Self::ExperimentAction,
    ) -> impl Future<Output = Result<Self::Exploration, Self::Error>> + Send;
    fn freeze_oracle_portfolio(
        &mut self,
        workspace: Self::Workspace,
        exploration: Self::Exploration,
    ) -> Result<Self::Portfolio, Self::Error>;
}

/// Runs Oracle Exploration as a readable architecture skeleton.
pub async fn run_oracle_exploration<S: OracleExplorationStages>(
    stages: &mut S,
    intent: &S::AdmittedIntent,
) -> Result<OracleExplorationRunOutcomeV1<S::Portfolio, S::Waiting>, S::Error> {
    let workspace = stages.freeze_oracle_workspace(intent).await?;
    let claims = stages.derive_oracle_claims(&workspace, intent)?;
    let work_items = stages.derive_oracle_work_items(&workspace, &claims)?;
    let mut exploration = stages.open_or_recover_oracle_exploration(&workspace, &work_items)?;

    loop {
        match stages.select_next_oracle_action(&exploration)? {
            OracleExplorationDirectiveV1::RunStrategy(action) => {
                exploration = stages
                    .run_oracle_strategy(&workspace, exploration, action)
                    .await?;
            }
            OracleExplorationDirectiveV1::AuthorizeControllerExperiment(action) => {
                exploration = stages
                    .authorize_and_run_oracle_experiment(&workspace, exploration, action)
                    .await?;
            }
            OracleExplorationDirectiveV1::AwaitObservation(waiting) => {
                return Ok(OracleExplorationRunOutcomeV1::AwaitingObservation(waiting));
            }
            OracleExplorationDirectiveV1::FreezePortfolio => {
                return Ok(OracleExplorationRunOutcomeV1::Portfolio(
                    stages.freeze_oracle_portfolio(workspace, exploration)?,
                ));
            }
            OracleExplorationDirectiveV1::BudgetExhausted => {
                return Ok(OracleExplorationRunOutcomeV1::BudgetExhausted);
            }
        }
    }
}

/// Frozen proposal preserves every resolved obligation, including unknowns and policy waivers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OraclePortfolioProposalV1 {
    schema_version: u16,
    workspace: ContentId<OracleWorkspaceArtifact>,
    ledger: ContentId<OracleExplorationLedgerArtifact>,
    entries: Vec<OracleObligationEntryV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OraclePortfolioProposalWire {
    schema_version: u16,
    workspace: ContentId<OracleWorkspaceArtifact>,
    ledger: ContentId<OracleExplorationLedgerArtifact>,
    entries: Vec<OracleObligationEntryV1>,
}

impl OraclePortfolioProposalV1 {
    pub fn freeze(ledger: &OracleExplorationLedgerV1) -> Result<Self, OracleFrameworkError> {
        if ledger
            .entries
            .iter()
            .any(|entry| !entry.resolution.is_terminal())
        {
            return Err(OracleFrameworkError::ExplorationIncomplete);
        }
        Ok(Self {
            schema_version: SCHEMA_V1,
            workspace: ledger.workspace,
            ledger: ledger.identity()?,
            entries: ledger.entries.clone(),
        })
    }

    #[must_use]
    pub const fn workspace(&self) -> ContentId<OracleWorkspaceArtifact> {
        self.workspace
    }

    #[must_use]
    pub fn entries(&self) -> &[OracleObligationEntryV1] {
        &self.entries
    }
    pub fn identity(
        &self,
    ) -> Result<ContentId<OraclePortfolioProposalArtifact>, OracleFrameworkError> {
        derive_id(self)
    }
    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        if self
            .entries
            .iter()
            .any(|entry| !entry.resolution.is_terminal())
        {
            return Err(OracleFrameworkError::ExplorationIncomplete);
        }
        let items: Vec<_> = self
            .entries
            .iter()
            .map(|entry| entry.item.clone())
            .collect();
        validate_strict(&items, "portfolio work items")
    }
}

impl TryFrom<OraclePortfolioProposalWire> for OraclePortfolioProposalV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OraclePortfolioProposalWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            workspace: wire.workspace,
            ledger: wire.ledger,
            entries: wire.entries,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OraclePortfolioProposalV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OraclePortfolioProposalWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Independent controls. No strategy or model-consensus alternative exists here.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleControlFamilyV1 {
    MechanismQualification,
    Honest,
    Mutant,
    Hidden,
    Bypass,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleAdmissionPolicyV1 {
    schema_version: u16,
    required_controls: Vec<OracleControlFamilyV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleAdmissionPolicyWire {
    schema_version: u16,
    required_controls: Vec<OracleControlFamilyV1>,
}

impl OracleAdmissionPolicyV1 {
    #[must_use]
    pub fn strict() -> Self {
        Self {
            schema_version: SCHEMA_V1,
            required_controls: vec![
                OracleControlFamilyV1::MechanismQualification,
                OracleControlFamilyV1::Honest,
                OracleControlFamilyV1::Mutant,
                OracleControlFamilyV1::Hidden,
                OracleControlFamilyV1::Bypass,
            ],
        }
    }
    #[must_use]
    pub fn required_controls(&self) -> &[OracleControlFamilyV1] {
        &self.required_controls
    }
    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleAdmissionPolicyArtifact>, OracleFrameworkError> {
        derive_id(self)
    }
}

impl TryFrom<OracleAdmissionPolicyWire> for OracleAdmissionPolicyV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OracleAdmissionPolicyWire) -> Result<Self, Self::Error> {
        require_v1(wire.schema_version)?;
        if wire.required_controls != Self::strict().required_controls {
            return Err(OracleFrameworkError::AdmissionPolicyDrift);
        }
        Ok(Self::strict())
    }
}

impl<'de> Deserialize<'de> for OracleAdmissionPolicyV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleAdmissionPolicyWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// One Controller-selected qualified mechanism for one independent control family.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleQualifiedMechanismRegistrationV1 {
    control: OracleControlFamilyV1,
    mechanism: ContentId<OracleQualifiedMechanismArtifact>,
    runner: ContentId<OracleControlRunnerArtifact>,
    qualification: ContentId<OracleMechanismQualificationReceiptArtifact>,
}

impl OracleQualifiedMechanismRegistrationV1 {
    #[must_use]
    pub const fn new(
        control: OracleControlFamilyV1,
        mechanism: ContentId<OracleQualifiedMechanismArtifact>,
        runner: ContentId<OracleControlRunnerArtifact>,
        qualification: ContentId<OracleMechanismQualificationReceiptArtifact>,
    ) -> Self {
        Self {
            control,
            mechanism,
            runner,
            qualification,
        }
    }

    #[must_use]
    pub const fn control(&self) -> OracleControlFamilyV1 {
        self.control
    }

    #[must_use]
    pub const fn mechanism(&self) -> ContentId<OracleQualifiedMechanismArtifact> {
        self.mechanism
    }

    #[must_use]
    pub const fn runner(&self) -> ContentId<OracleControlRunnerArtifact> {
        self.runner
    }

    #[must_use]
    pub const fn qualification(&self) -> ContentId<OracleMechanismQualificationReceiptArtifact> {
        self.qualification
    }

    pub fn validate_qualification(
        &self,
        receipt: &crate::OracleMechanismQualificationReceiptV1,
    ) -> Result<(), OracleFrameworkError> {
        if receipt.control() != self.control
            || receipt.mechanism() != self.mechanism
            || receipt.runner() != self.runner
            || receipt
                .identity()
                .map_err(|error| OracleFrameworkError::Codec(error.to_string()))?
                != self.qualification
        {
            return Err(OracleFrameworkError::AdmissionMechanismCatalogDrift);
        }
        Ok(())
    }
}

/// Exact qualified mechanism inventory frozen before any admission control may run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleAdmissionMechanismCatalogV1 {
    schema_version: u16,
    mechanisms: Vec<OracleQualifiedMechanismRegistrationV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleAdmissionMechanismCatalogWire {
    schema_version: u16,
    mechanisms: Vec<OracleQualifiedMechanismRegistrationV1>,
}

impl OracleAdmissionMechanismCatalogV1 {
    pub fn new(
        mut mechanisms: Vec<OracleQualifiedMechanismRegistrationV1>,
    ) -> Result<Self, OracleFrameworkError> {
        mechanisms.sort_by_key(OracleQualifiedMechanismRegistrationV1::control);
        let value = Self {
            schema_version: SCHEMA_V1,
            mechanisms,
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub fn mechanisms(&self) -> &[OracleQualifiedMechanismRegistrationV1] {
        &self.mechanisms
    }

    #[must_use]
    pub fn mechanism(
        &self,
        control: OracleControlFamilyV1,
    ) -> Option<ContentId<OracleQualifiedMechanismArtifact>> {
        self.mechanisms
            .iter()
            .find(|registration| registration.control == control)
            .map(|registration| registration.mechanism)
    }

    #[must_use]
    pub fn registration(
        &self,
        control: OracleControlFamilyV1,
    ) -> Option<&OracleQualifiedMechanismRegistrationV1> {
        self.mechanisms
            .iter()
            .find(|registration| registration.control == control)
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleAdmissionMechanismCatalogArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        let controls = self
            .mechanisms
            .iter()
            .map(OracleQualifiedMechanismRegistrationV1::control)
            .collect::<Vec<_>>();
        if controls != OracleAdmissionPolicyV1::strict().required_controls {
            return Err(OracleFrameworkError::AdmissionMechanismCatalogDrift);
        }
        Ok(())
    }
}

impl TryFrom<OracleAdmissionMechanismCatalogWire> for OracleAdmissionMechanismCatalogV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleAdmissionMechanismCatalogWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            mechanisms: wire.mechanisms,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleAdmissionMechanismCatalogV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleAdmissionMechanismCatalogWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// One exact item × control × qualified mechanism obligation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleControlObligationV1 {
    item: ContentId<OracleWorkItemArtifact>,
    control: OracleControlFamilyV1,
    mechanism: ContentId<OracleQualifiedMechanismArtifact>,
}

impl OracleControlObligationV1 {
    #[must_use]
    pub const fn item(&self) -> ContentId<OracleWorkItemArtifact> {
        self.item
    }

    #[must_use]
    pub const fn control(&self) -> OracleControlFamilyV1 {
        self.control
    }

    #[must_use]
    pub const fn mechanism(&self) -> ContentId<OracleQualifiedMechanismArtifact> {
        self.mechanism
    }
}

/// Frozen mechanical Admission authority derived from one exact portfolio.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleAdmissionAttemptV1 {
    schema_version: u16,
    proposal: ContentId<OraclePortfolioProposalArtifact>,
    policy: ContentId<OracleAdmissionPolicyArtifact>,
    mechanisms: ContentId<OracleAdmissionMechanismCatalogArtifact>,
    required_controls: Vec<OracleControlObligationV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleAdmissionAttemptWire {
    schema_version: u16,
    proposal: ContentId<OraclePortfolioProposalArtifact>,
    policy: ContentId<OracleAdmissionPolicyArtifact>,
    mechanisms: ContentId<OracleAdmissionMechanismCatalogArtifact>,
    required_controls: Vec<OracleControlObligationV1>,
}

impl OracleAdmissionAttemptV1 {
    pub fn new(
        proposal: &OraclePortfolioProposalV1,
        policy: &OracleAdmissionPolicyV1,
        mechanisms: &OracleAdmissionMechanismCatalogV1,
    ) -> Result<Self, OracleFrameworkError> {
        let mut required_controls = Vec::new();
        for entry in proposal.entries() {
            for control in policy.required_controls() {
                required_controls.push(OracleControlObligationV1 {
                    item: entry.item().identity()?,
                    control: *control,
                    mechanism: mechanisms
                        .mechanism(*control)
                        .ok_or(OracleFrameworkError::AdmissionMechanismCatalogDrift)?,
                });
            }
        }
        required_controls.sort_by(|left, right| {
            left.item
                .to_wire()
                .cmp(&right.item.to_wire())
                .then_with(|| left.control.cmp(&right.control))
        });
        let value = Self {
            schema_version: SCHEMA_V1,
            proposal: proposal.identity()?,
            policy: policy.identity()?,
            mechanisms: mechanisms.identity()?,
            required_controls,
        };
        value.validate_structure()?;
        Ok(value)
    }

    #[must_use]
    pub const fn proposal(&self) -> ContentId<OraclePortfolioProposalArtifact> {
        self.proposal
    }

    #[must_use]
    pub const fn policy(&self) -> ContentId<OracleAdmissionPolicyArtifact> {
        self.policy
    }

    #[must_use]
    pub const fn mechanisms(&self) -> ContentId<OracleAdmissionMechanismCatalogArtifact> {
        self.mechanisms
    }

    #[must_use]
    pub fn required_controls(&self) -> &[OracleControlObligationV1] {
        &self.required_controls
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleAdmissionAttemptArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    fn validate_against(
        &self,
        proposal: &OraclePortfolioProposalV1,
        policy: &OracleAdmissionPolicyV1,
        mechanisms: &OracleAdmissionMechanismCatalogV1,
    ) -> Result<(), OracleFrameworkError> {
        if self != &Self::new(proposal, policy, mechanisms)? {
            return Err(OracleFrameworkError::AdmissionAttemptBindingMismatch);
        }
        Ok(())
    }

    fn validate_structure(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        let controls = OracleAdmissionPolicyV1::strict();
        for chunk in self
            .required_controls
            .chunks(controls.required_controls.len())
        {
            if chunk.len() != controls.required_controls.len()
                || chunk
                    .iter()
                    .map(OracleControlObligationV1::control)
                    .collect::<Vec<_>>()
                    != controls.required_controls
                || chunk.windows(2).any(|pair| pair[0].item != pair[1].item)
            {
                return Err(OracleFrameworkError::AdmissionAttemptBindingMismatch);
            }
        }
        if self.required_controls.windows(2).any(|pair| {
            pair[0].item.to_wire() > pair[1].item.to_wire()
                || (pair[0].item == pair[1].item && pair[0].control >= pair[1].control)
        }) {
            return Err(OracleFrameworkError::AdmissionAttemptBindingMismatch);
        }
        Ok(())
    }
}

impl TryFrom<OracleAdmissionAttemptWire> for OracleAdmissionAttemptV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleAdmissionAttemptWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            proposal: wire.proposal,
            policy: wire.policy,
            mechanisms: wire.mechanisms,
            required_controls: wire.required_controls,
        };
        value.validate_structure()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleAdmissionAttemptV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleAdmissionAttemptWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleControlResultV1 {
    Passed,
    Failed,
    Unavailable,
}

/// Controller-validated receipt for one exact portfolio work item and control family.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleControlReceiptV1 {
    proposal: ContentId<OraclePortfolioProposalArtifact>,
    item: ContentId<OracleWorkItemArtifact>,
    control: OracleControlFamilyV1,
    mechanism: ContentId<OracleQualifiedMechanismArtifact>,
    receipt: ContentId<TrustedOracleControlReceiptArtifact>,
    result: OracleControlResultV1,
}

impl OracleControlReceiptV1 {
    #[must_use]
    pub const fn new(
        proposal: ContentId<OraclePortfolioProposalArtifact>,
        item: ContentId<OracleWorkItemArtifact>,
        control: OracleControlFamilyV1,
        mechanism: ContentId<OracleQualifiedMechanismArtifact>,
        receipt: ContentId<TrustedOracleControlReceiptArtifact>,
        result: OracleControlResultV1,
    ) -> Self {
        Self {
            proposal,
            item,
            control,
            mechanism,
            receipt,
            result,
        }
    }

    pub fn from_trusted_observation(
        proposal: ContentId<OraclePortfolioProposalArtifact>,
        run: &OracleControlRunV1,
        observation: &TrustedOracleControlObservationV1,
    ) -> Result<Self, OracleFrameworkError> {
        if observation.run()
            != run
                .identity()
                .map_err(|error| OracleFrameworkError::Codec(error.to_string()))?
        {
            return Err(OracleFrameworkError::ReceiptBindingMismatch);
        }
        Ok(Self {
            proposal,
            item: run.obligation().item(),
            control: run.obligation().control(),
            mechanism: run.obligation().mechanism(),
            receipt: observation
                .identity()
                .map_err(|error| OracleFrameworkError::Codec(error.to_string()))?,
            result: observation.result(),
        })
    }

    #[must_use]
    pub const fn proposal(&self) -> ContentId<OraclePortfolioProposalArtifact> {
        self.proposal
    }

    #[must_use]
    pub const fn item(&self) -> ContentId<OracleWorkItemArtifact> {
        self.item
    }

    #[must_use]
    pub const fn control(&self) -> OracleControlFamilyV1 {
        self.control
    }

    #[must_use]
    pub const fn mechanism(&self) -> ContentId<OracleQualifiedMechanismArtifact> {
        self.mechanism
    }

    #[must_use]
    pub const fn receipt(&self) -> ContentId<TrustedOracleControlReceiptArtifact> {
        self.receipt
    }

    #[must_use]
    pub const fn result(&self) -> OracleControlResultV1 {
        self.result
    }
}

/// Exact trusted receipt set submitted for one frozen Admission attempt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleAdmissionEvidenceV1 {
    schema_version: u16,
    attempt: ContentId<OracleAdmissionAttemptArtifact>,
    receipts: Vec<OracleControlReceiptV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleAdmissionEvidenceWire {
    schema_version: u16,
    attempt: ContentId<OracleAdmissionAttemptArtifact>,
    receipts: Vec<OracleControlReceiptV1>,
}

impl OracleAdmissionEvidenceV1 {
    pub fn new(
        attempt: &OracleAdmissionAttemptV1,
        mut receipts: Vec<OracleControlReceiptV1>,
    ) -> Result<Self, OracleFrameworkError> {
        receipts.sort_by(|left, right| {
            left.item
                .to_wire()
                .cmp(&right.item.to_wire())
                .then_with(|| left.control.cmp(&right.control))
        });
        let value = Self {
            schema_version: SCHEMA_V1,
            attempt: attempt.identity()?,
            receipts,
        };
        value.validate_against(attempt)?;
        Ok(value)
    }

    #[must_use]
    pub const fn attempt(&self) -> ContentId<OracleAdmissionAttemptArtifact> {
        self.attempt
    }

    #[must_use]
    pub fn receipts(&self) -> &[OracleControlReceiptV1] {
        &self.receipts
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleAdmissionEvidenceArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    fn validate_against(
        &self,
        attempt: &OracleAdmissionAttemptV1,
    ) -> Result<(), OracleFrameworkError> {
        self.validate_structure()?;
        if self.attempt != attempt.identity()?
            || self.receipts.iter().any(|receipt| {
                receipt.proposal != attempt.proposal
                    || !attempt.required_controls.iter().any(|obligation| {
                        obligation.item == receipt.item
                            && obligation.control == receipt.control
                            && obligation.mechanism == receipt.mechanism
                    })
            })
        {
            return Err(OracleFrameworkError::AdmissionEvidenceBindingMismatch);
        }
        Ok(())
    }

    fn validate_structure(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        let receipt_identities = self
            .receipts
            .iter()
            .map(OracleControlReceiptV1::receipt)
            .collect::<HashSet<_>>();
        if receipt_identities.len() != self.receipts.len()
            || self.receipts.windows(2).any(|pair| {
                pair[0].item.to_wire() > pair[1].item.to_wire()
                    || (pair[0].item == pair[1].item && pair[0].control >= pair[1].control)
            })
        {
            return Err(OracleFrameworkError::DuplicateControlReceipt);
        }
        Ok(())
    }
}

impl TryFrom<OracleAdmissionEvidenceWire> for OracleAdmissionEvidenceV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleAdmissionEvidenceWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            attempt: wire.attempt,
            receipts: wire.receipts,
        };
        value.validate_structure()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleAdmissionEvidenceV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleAdmissionEvidenceWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleClaimAdmissionStatusV1 {
    Admitted,
    Partial,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleClaimAdmissionV1 {
    claim: ContentId<OracleClaimArtifact>,
    status: OracleClaimAdmissionStatusV1,
    admitted_items: Vec<ContentId<OracleWorkItemArtifact>>,
    unresolved_items: Vec<ContentId<OracleWorkItemArtifact>>,
    rejected_items: Vec<ContentId<OracleWorkItemArtifact>>,
}

impl OracleClaimAdmissionV1 {
    #[must_use]
    pub const fn claim(&self) -> ContentId<OracleClaimArtifact> {
        self.claim
    }

    #[must_use]
    pub const fn status(&self) -> OracleClaimAdmissionStatusV1 {
        self.status
    }

    #[must_use]
    pub fn admitted_items(&self) -> &[ContentId<OracleWorkItemArtifact>] {
        &self.admitted_items
    }

    #[must_use]
    pub fn unresolved_items(&self) -> &[ContentId<OracleWorkItemArtifact>] {
        &self.unresolved_items
    }

    #[must_use]
    pub fn rejected_items(&self) -> &[ContentId<OracleWorkItemArtifact>] {
        &self.rejected_items
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        validate_content_id_order(&self.admitted_items, "admitted Oracle work items")?;
        validate_content_id_order(&self.unresolved_items, "unresolved Oracle work items")?;
        validate_content_id_order(&self.rejected_items, "rejected Oracle work items")?;
        let all: HashSet<_> = self
            .admitted_items
            .iter()
            .chain(&self.unresolved_items)
            .chain(&self.rejected_items)
            .copied()
            .collect();
        if all.len()
            != self.admitted_items.len() + self.unresolved_items.len() + self.rejected_items.len()
        {
            return Err(OracleFrameworkError::AdmissionOutcomeInvalid);
        }
        let status_matches = match self.status {
            OracleClaimAdmissionStatusV1::Admitted => {
                !self.admitted_items.is_empty()
                    && self.unresolved_items.is_empty()
                    && self.rejected_items.is_empty()
            }
            OracleClaimAdmissionStatusV1::Partial => {
                !self.unresolved_items.is_empty() && self.rejected_items.is_empty()
            }
            OracleClaimAdmissionStatusV1::Rejected => !self.rejected_items.is_empty(),
        };
        if !status_matches {
            return Err(OracleFrameworkError::AdmissionOutcomeInvalid);
        }
        Ok(())
    }
}

/// Model-free claim portfolio recomputed only from the proposal and exact trusted receipts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleAdmissionOutcomeV1 {
    schema_version: u16,
    attempt: ContentId<OracleAdmissionAttemptArtifact>,
    evidence: ContentId<OracleAdmissionEvidenceArtifact>,
    proposal: ContentId<OraclePortfolioProposalArtifact>,
    policy: ContentId<OracleAdmissionPolicyArtifact>,
    claims: Vec<OracleClaimAdmissionV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleAdmissionOutcomeWire {
    schema_version: u16,
    attempt: ContentId<OracleAdmissionAttemptArtifact>,
    evidence: ContentId<OracleAdmissionEvidenceArtifact>,
    proposal: ContentId<OraclePortfolioProposalArtifact>,
    policy: ContentId<OracleAdmissionPolicyArtifact>,
    claims: Vec<OracleClaimAdmissionV1>,
}

impl OracleAdmissionOutcomeV1 {
    #[must_use]
    pub const fn attempt(&self) -> ContentId<OracleAdmissionAttemptArtifact> {
        self.attempt
    }

    #[must_use]
    pub const fn evidence(&self) -> ContentId<OracleAdmissionEvidenceArtifact> {
        self.evidence
    }

    #[must_use]
    pub const fn proposal(&self) -> ContentId<OraclePortfolioProposalArtifact> {
        self.proposal
    }

    #[must_use]
    pub const fn policy(&self) -> ContentId<OracleAdmissionPolicyArtifact> {
        self.policy
    }

    #[must_use]
    pub fn claims(&self) -> &[OracleClaimAdmissionV1] {
        &self.claims
    }
    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleAdmissionOutcomeArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        if self.claims.is_empty()
            || self
                .claims
                .windows(2)
                .any(|pair| pair[0].claim.to_wire() >= pair[1].claim.to_wire())
        {
            return Err(OracleFrameworkError::AdmissionOutcomeInvalid);
        }
        for claim in &self.claims {
            claim.validate()?;
        }
        Ok(())
    }
}

impl TryFrom<OracleAdmissionOutcomeWire> for OracleAdmissionOutcomeV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OracleAdmissionOutcomeWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            attempt: wire.attempt,
            evidence: wire.evidence,
            proposal: wire.proposal,
            policy: wire.policy,
            claims: wire.claims,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleAdmissionOutcomeV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleAdmissionOutcomeWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

type AdmissionBuckets = (
    Vec<ContentId<OracleWorkItemArtifact>>,
    Vec<ContentId<OracleWorkItemArtifact>>,
    Vec<ContentId<OracleWorkItemArtifact>>,
);

/// Mechanically recomputes admitted/partial/rejected claims. Missing controls remain partial.
pub fn recompute_oracle_admission(
    proposal: &OraclePortfolioProposalV1,
    policy: &OracleAdmissionPolicyV1,
    mechanisms: &OracleAdmissionMechanismCatalogV1,
    attempt: &OracleAdmissionAttemptV1,
    evidence: &OracleAdmissionEvidenceV1,
) -> Result<OracleAdmissionOutcomeV1, OracleFrameworkError> {
    attempt.validate_against(proposal, policy, mechanisms)?;
    evidence.validate_against(attempt)?;
    let receipts = evidence.receipts();
    let proposal_id = proposal.identity()?;
    let known_items: HashSet<_> = proposal
        .entries
        .iter()
        .map(|entry| entry.item.identity())
        .collect::<Result<_, _>>()?;
    let mut receipt_map = HashMap::new();
    for receipt in receipts {
        if receipt.proposal != proposal_id || !known_items.contains(&receipt.item) {
            return Err(OracleFrameworkError::ReceiptBindingMismatch);
        }
        let key = (receipt.item, receipt.control);
        if receipt_map.insert(key, receipt).is_some() {
            return Err(OracleFrameworkError::DuplicateControlReceipt);
        }
    }

    let mut by_claim: Vec<(ContentId<OracleClaimArtifact>, AdmissionBuckets)> = Vec::new();
    for entry in &proposal.entries {
        let item_id = entry.item.identity()?;
        let buckets = if let Some((_, buckets)) = by_claim
            .iter_mut()
            .find(|(claim, _)| *claim == entry.item.claim)
        {
            buckets
        } else {
            let new_index = by_claim.len();
            by_claim.push((entry.item.claim, AdmissionBuckets::default()));
            &mut by_claim[new_index].1
        };
        if !matches!(
            entry.resolution,
            OracleObligationResolutionV1::Contributed { .. }
        ) {
            buckets.1.push(item_id);
            continue;
        }
        let mut missing = false;
        let mut failed = false;
        for control in policy.required_controls() {
            match receipt_map
                .get(&(item_id, *control))
                .map(|receipt| receipt.result)
            {
                Some(OracleControlResultV1::Passed) => {}
                Some(OracleControlResultV1::Failed) => failed = true,
                Some(OracleControlResultV1::Unavailable) | None => missing = true,
            }
        }
        if failed {
            buckets.2.push(item_id);
        } else if missing {
            buckets.1.push(item_id);
        } else {
            buckets.0.push(item_id);
        }
    }

    let mut claims = by_claim
        .into_iter()
        .map(
            |(claim, (mut admitted_items, mut unresolved_items, mut rejected_items))| {
                admitted_items.sort_by_key(ContentId::to_wire);
                unresolved_items.sort_by_key(ContentId::to_wire);
                rejected_items.sort_by_key(ContentId::to_wire);
                let status = if !rejected_items.is_empty() {
                    OracleClaimAdmissionStatusV1::Rejected
                } else if unresolved_items.is_empty() {
                    OracleClaimAdmissionStatusV1::Admitted
                } else {
                    OracleClaimAdmissionStatusV1::Partial
                };
                OracleClaimAdmissionV1 {
                    claim,
                    status,
                    admitted_items,
                    unresolved_items,
                    rejected_items,
                }
            },
        )
        .collect::<Vec<_>>();
    claims.sort_by_key(|claim| claim.claim.to_wire());
    let outcome = OracleAdmissionOutcomeV1 {
        schema_version: SCHEMA_V1,
        attempt: attempt.identity()?,
        evidence: evidence.identity()?,
        proposal: proposal_id,
        policy: policy.identity()?,
        claims,
    };
    outcome.validate()?;
    Ok(outcome)
}

/// Ports for independent, model-free Oracle Admission.
pub trait IndependentOracleAdmissionStages: Send {
    type Error: Send;
    type PortfolioProposal: Send + Sync;
    type FrozenPortfolio: Send + Sync;
    type RequiredControls: Send + Sync;
    type QualifiedMechanisms: Send + Sync;
    type ControlReceipts: Send + Sync;
    type RecomputedClaims: Send;
    type PublishedOutcome: Send;

    fn freeze_portfolio_candidate(
        &mut self,
        proposal: Self::PortfolioProposal,
    ) -> Result<Self::FrozenPortfolio, Self::Error>;
    fn derive_required_oracle_controls(
        &mut self,
        portfolio: &Self::FrozenPortfolio,
    ) -> Result<Self::RequiredControls, Self::Error>;
    fn qualify_oracle_mechanisms(
        &mut self,
        portfolio: &Self::FrozenPortfolio,
        controls: &Self::RequiredControls,
    ) -> impl Future<Output = Result<Self::QualifiedMechanisms, Self::Error>> + Send;
    fn execute_authorized_oracle_controls(
        &mut self,
        portfolio: &Self::FrozenPortfolio,
        controls: &Self::RequiredControls,
        mechanisms: &Self::QualifiedMechanisms,
    ) -> impl Future<Output = Result<Self::ControlReceipts, Self::Error>> + Send;
    fn recompute_oracle_claim_portfolio(
        &mut self,
        portfolio: &Self::FrozenPortfolio,
        controls: &Self::RequiredControls,
        mechanisms: &Self::QualifiedMechanisms,
        receipts: &Self::ControlReceipts,
    ) -> Result<Self::RecomputedClaims, Self::Error>;
    fn publish_oracle_admission_outcome(
        &mut self,
        portfolio: Self::FrozenPortfolio,
        claims: Self::RecomputedClaims,
    ) -> Result<Self::PublishedOutcome, Self::Error>;
}

/// Runs independent admission without a model-vote or proposal-author shortcut.
pub async fn run_independent_oracle_admission<S: IndependentOracleAdmissionStages>(
    stages: &mut S,
    proposal: S::PortfolioProposal,
) -> Result<S::PublishedOutcome, S::Error> {
    let portfolio = stages.freeze_portfolio_candidate(proposal)?;
    let controls = stages.derive_required_oracle_controls(&portfolio)?;
    let mechanisms = stages
        .qualify_oracle_mechanisms(&portfolio, &controls)
        .await?;
    let receipts = stages
        .execute_authorized_oracle_controls(&portfolio, &controls, &mechanisms)
        .await?;
    let claims =
        stages.recompute_oracle_claim_portfolio(&portfolio, &controls, &mechanisms, &receipts)?;
    stages.publish_oracle_admission_outcome(portfolio, claims)
}

#[derive(Debug, Error)]
pub enum OracleFrameworkError {
    #[error("oracle framework accepts only current schema version 1")]
    UnsupportedSchema,
    #[error("invalid {0}")]
    InvalidLabel(&'static str),
    #[error("{0} must not be empty")]
    Empty(&'static str),
    #[error("{0} must be positive")]
    NonPositive(&'static str),
    #[error("{0} is not a strict canonical set")]
    NonCanonical(&'static str),
    #[error("coverage policy mandatory concerns drifted")]
    CoveragePolicyDrift,
    #[error("admission policy required controls drifted")]
    AdmissionPolicyDrift,
    #[error("Oracle Admission qualified mechanism catalog drifted")]
    AdmissionMechanismCatalogDrift,
    #[error("Oracle Admission attempt changed its portfolio, policy, mechanism, or obligations")]
    AdmissionAttemptBindingMismatch,
    #[error("Oracle Admission evidence is not authorized by the exact attempt")]
    AdmissionEvidenceBindingMismatch,
    #[error("strategy kind and executor disagree")]
    StrategyExecutorMismatch,
    #[error("Oracle strategy tool catalog changed the current-V1 capability surface")]
    StrategyToolCatalogDrift,
    #[error("oracle work item plane does not match its concern")]
    WorkItemPlaneMismatch,
    #[error("oracle exploration revision overflowed")]
    RevisionOverflow,
    #[error("oracle exploration revision and parent lineage disagree")]
    RevisionLineageMismatch,
    #[error("oracle exploration strategy budget is exhausted")]
    StrategyBudgetExhausted,
    #[error("oracle exploration experiment budget is exhausted")]
    ExperimentBudgetExhausted,
    #[error("strategy is not eligible for the exact work item")]
    IneligibleStrategy,
    #[error("oracle exploration ledger transition is invalid")]
    InvalidLedgerTransition,
    #[error("oracle strategy run binding changed")]
    StrategyRunBindingMismatch,
    #[error("oracle experiment request binding changed")]
    ExperimentBindingMismatch,
    #[error("oracle work item is not in the exploration ledger")]
    UnknownWorkItem,
    #[error("oracle experiment observation is duplicated")]
    DuplicateObservation,
    #[error("oracle experiment observation binding changed")]
    ObservationBindingMismatch,
    #[error("Oracle portfolio element cell or strategy-run binding changed")]
    PortfolioElementBindingMismatch,
    #[error("coverage-gap material cannot be mixed with positive Oracle contributions")]
    MixedCoverageGapContribution,
    #[error("no strategy implements {plane:?}/{concern:?}/{role:?}")]
    MissingStrategy {
        plane: OraclePlaneV1,
        concern: OracleConcernV1,
        role: OracleStrategyRoleV1,
    },
    #[error("Oracle Exploration still has non-terminal obligations")]
    ExplorationIncomplete,
    #[error("control receipt is bound to another portfolio proposal")]
    ReceiptBindingMismatch,
    #[error("duplicate control receipt for one work item and family")]
    DuplicateControlReceipt,
    #[error("Oracle admission outcome structure is inconsistent")]
    AdmissionOutcomeInvalid,
    #[error("oracle framework codec failed: {0}")]
    Codec(String),
    #[error(transparent)]
    Content(#[from] ContentStoreError),
}

fn require_v1(version: u16) -> Result<(), OracleFrameworkError> {
    if version == SCHEMA_V1 {
        Ok(())
    } else {
        Err(OracleFrameworkError::UnsupportedSchema)
    }
}

fn validate_strict<T: Ord>(values: &[T], field: &'static str) -> Result<(), OracleFrameworkError> {
    if values.is_empty() {
        return Err(OracleFrameworkError::Empty(field));
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(OracleFrameworkError::NonCanonical(field));
    }
    Ok(())
}

fn validate_content_ids<T: ContentType>(
    values: &[ContentId<T>],
    field: &'static str,
) -> Result<(), OracleFrameworkError> {
    if values.is_empty() {
        return Err(OracleFrameworkError::Empty(field));
    }
    if values
        .windows(2)
        .any(|pair| pair[0].to_wire() >= pair[1].to_wire())
    {
        return Err(OracleFrameworkError::NonCanonical(field));
    }
    Ok(())
}

fn validate_content_id_order<T: ContentType>(
    values: &[ContentId<T>],
    field: &'static str,
) -> Result<(), OracleFrameworkError> {
    if values
        .windows(2)
        .any(|pair| pair[0].to_wire() >= pair[1].to_wire())
    {
        return Err(OracleFrameworkError::NonCanonical(field));
    }
    Ok(())
}

fn derive_id<T: Serialize, A: ContentType>(
    value: &T,
) -> Result<ContentId<A>, OracleFrameworkError> {
    let bytes = cairn_codec::to_vec(value)
        .map_err(|error| OracleFrameworkError::Codec(error.to_string()))?;
    ContentId::derive(&bytes).map_err(|error| OracleFrameworkError::Codec(error.to_string()))
}

/// Archives one canonical framework artifact.
pub fn archive_oracle_framework_artifact<A: ContentType, S: ContentStore>(
    store: &mut S,
    value: &impl Serialize,
) -> Result<ContentId<A>, OracleFrameworkError> {
    let bytes = cairn_codec::to_vec(value)
        .map_err(|error| OracleFrameworkError::Codec(error.to_string()))?;
    Ok(store.put::<A>(&mut Cursor::new(bytes))?.content_id)
}

#[cfg(test)]
mod tests {
    use std::future::ready;

    use super::*;

    fn id<A: ContentType>(label: &str) -> ContentId<A> {
        ContentId::derive(label.as_bytes()).expect("id")
    }

    fn claim(label: &str) -> ContentId<OracleClaimArtifact> {
        id(label)
    }

    fn admission_context(
        proposal: &OraclePortfolioProposalV1,
    ) -> (
        OracleAdmissionPolicyV1,
        OracleAdmissionMechanismCatalogV1,
        OracleAdmissionAttemptV1,
    ) {
        let policy = OracleAdmissionPolicyV1::strict();
        let mechanisms = OracleAdmissionMechanismCatalogV1::new(
            policy
                .required_controls()
                .iter()
                .map(|control| {
                    OracleQualifiedMechanismRegistrationV1::new(
                        *control,
                        id(match control {
                            OracleControlFamilyV1::MechanismQualification => {
                                "mechanism qualification"
                            }
                            OracleControlFamilyV1::Honest => "honest mechanism",
                            OracleControlFamilyV1::Mutant => "mutant mechanism",
                            OracleControlFamilyV1::Hidden => "hidden mechanism",
                            OracleControlFamilyV1::Bypass => "bypass mechanism",
                        }),
                        id(match control {
                            OracleControlFamilyV1::MechanismQualification => {
                                "mechanism qualification runner"
                            }
                            OracleControlFamilyV1::Honest => "honest runner",
                            OracleControlFamilyV1::Mutant => "mutant runner",
                            OracleControlFamilyV1::Hidden => "hidden runner",
                            OracleControlFamilyV1::Bypass => "bypass runner",
                        }),
                        id(match control {
                            OracleControlFamilyV1::MechanismQualification => {
                                "mechanism qualification receipt"
                            }
                            OracleControlFamilyV1::Honest => "honest qualification receipt",
                            OracleControlFamilyV1::Mutant => "mutant qualification receipt",
                            OracleControlFamilyV1::Hidden => "hidden qualification receipt",
                            OracleControlFamilyV1::Bypass => "bypass qualification receipt",
                        }),
                    )
                })
                .collect(),
        )
        .expect("mechanism catalog");
        let attempt = OracleAdmissionAttemptV1::new(proposal, &policy, &mechanisms)
            .expect("admission attempt");
        (policy, mechanisms, attempt)
    }

    fn all_concerns_registration(
        role: OracleStrategyRoleV1,
        name: &str,
    ) -> OracleStrategyRegistrationV1 {
        OracleStrategyRegistrationV1::new(
            OracleStrategyName::new(name).expect("name"),
            OracleStrategyKindV1::DeterministicAnalyzer,
            OracleStrategyExecutorV1::Deterministic {
                implementation: id(name),
            },
            vec![role],
            OracleCoveragePolicyV1::new(
                OracleCoverageProfileV1::Correctness,
                OracleAdversarialPolicyV1::NotRequired,
            )
            .concerns,
        )
        .expect("registration")
    }

    #[test]
    fn controller_expands_every_claim_concern_and_role_without_agent_discretion() {
        let policy = OracleCoveragePolicyV1::new(
            OracleCoverageProfileV1::Correctness,
            OracleAdversarialPolicyV1::RequiredForEveryConcern,
        );
        let mut claims = vec![claim("claim-a"), claim("claim-b")];
        claims.sort_by_key(ContentId::to_wire);
        let items = derive_oracle_work_items(&claims, &policy).expect("items");

        assert_eq!(items.len(), claims.len() * policy.concerns().len() * 2);
        for claim in claims {
            for concern in policy.concerns() {
                assert!(items.contains(&OracleWorkItemV1::new(
                    claim,
                    *concern,
                    OracleStrategyRoleV1::Synthesis
                )));
                assert!(items.contains(&OracleWorkItemV1::new(
                    claim,
                    *concern,
                    OracleStrategyRoleV1::Adversarial
                )));
            }
        }
        assert!(
            policy
                .concerns()
                .contains(&OracleConcernV1::CrossPlaneInvariants)
        );
        assert!(
            policy
                .concerns()
                .contains(&OracleConcernV1::UncataloguedRiskDiscovery)
        );
    }

    #[test]
    fn persisted_policy_cannot_omit_one_mandatory_plane_concern() {
        let policy = OracleCoveragePolicyV1::new(
            OracleCoverageProfileV1::Correctness,
            OracleAdversarialPolicyV1::NotRequired,
        );
        let mut json = serde_json::to_value(policy).expect("json");
        json["concerns"].as_array_mut().expect("concerns").pop();
        assert!(serde_json::from_value::<OracleCoveragePolicyV1>(json).is_err());
    }

    #[test]
    fn exploration_cannot_open_when_any_work_item_has_no_strategy() {
        let policy = OracleCoveragePolicyV1::new(
            OracleCoverageProfileV1::Correctness,
            OracleAdversarialPolicyV1::RequiredForEveryConcern,
        );
        let items = derive_oracle_work_items(&[claim("claim-a")], &policy).expect("items");
        let catalog = OracleStrategyCatalogV1::new(vec![all_concerns_registration(
            OracleStrategyRoleV1::Synthesis,
            "deterministic-synthesis",
        )])
        .expect("catalog");

        assert!(matches!(
            OracleExplorationLedgerV1::open(id("workspace"), items, &catalog),
            Err(OracleFrameworkError::MissingStrategy {
                role: OracleStrategyRoleV1::Adversarial,
                ..
            })
        ));
    }

    #[test]
    fn incomplete_ledger_cannot_be_frozen_as_portfolio() {
        let policy = OracleCoveragePolicyV1::new(
            OracleCoverageProfileV1::Correctness,
            OracleAdversarialPolicyV1::NotRequired,
        );
        let items = derive_oracle_work_items(&[claim("claim-a")], &policy).expect("items");
        let catalog = OracleStrategyCatalogV1::new(vec![all_concerns_registration(
            OracleStrategyRoleV1::Synthesis,
            "deterministic-synthesis",
        )])
        .expect("catalog");
        let ledger =
            OracleExplorationLedgerV1::open(id("workspace"), items, &catalog).expect("ledger");

        assert!(matches!(
            OraclePortfolioProposalV1::freeze(&ledger),
            Err(OracleFrameworkError::ExplorationIncomplete)
        ));
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one linear control keeps the exact six-revision authority chain and drift probes visible"
    )]
    fn experiment_roundtrip_is_exact_and_each_durable_revision_keeps_lineage() {
        let policy = OracleCoveragePolicyV1::new(
            OracleCoverageProfileV1::Correctness,
            OracleAdversarialPolicyV1::NotRequired,
        );
        let items = derive_oracle_work_items(&[claim("claim-a")], &policy).expect("items");
        let catalog = OracleStrategyCatalogV1::new(vec![all_concerns_registration(
            OracleStrategyRoleV1::Synthesis,
            "deterministic-synthesis",
        )])
        .expect("catalog");
        let budget = OracleExplorationBudgetV1 {
            strategy_runs: OracleStrategyRunLimit::new(32).expect("run limit"),
            experiments: OracleExperimentLimit::new(8).expect("experiment limit"),
        };
        let experiment_tools = id::<OracleExperimentToolCatalogArtifact>("experiment tools");
        let workspace = OracleWorkspaceV1::new(&OracleWorkspaceInput {
            task_id: TaskId::new(),
            admitted_intent: id("admitted intent"),
            sir_input: id("sir input"),
            sir_task_bundle: id("sir task"),
            source: id("source"),
            documentation: id("documentation"),
            build_and_tests: id("build and tests"),
            knowledge: id("knowledge"),
            research_tools: id("research tools"),
            experiment_tools,
            capability_grant: id("capability grant"),
            coverage_policy: policy.identity().expect("policy"),
            strategy_catalog: catalog.identity().expect("catalog"),
            budget,
        });
        let workspace_id = workspace.identity().expect("workspace");
        let ledger = OracleExplorationLedgerV1::open(workspace_id, items, &catalog).expect("open");
        let work_item = ledger.entries()[0].item.clone();
        let item = work_item.identity().expect("item");
        let strategy = OracleStrategyName::new("deterministic-synthesis").expect("strategy");
        let run = OracleStrategyRunV1::new(workspace_id, &work_item, strategy, &catalog)
            .expect("strategy run");
        let run_id = run.identity().expect("run");
        let request = OracleExperimentRequestV1::new(
            item,
            run_id,
            experiment_tools,
            OracleExperimentOperationName::new("execute-probe").expect("operation"),
            id("arguments"),
        );
        let receipt = TrustedOracleWorkerReceiptV1::new(
            request.identity().expect("request"),
            id("job contract"),
            id("execution receipt"),
        );

        let mut drifted_run_json = serde_json::to_value(&run).expect("run json");
        drifted_run_json["executor"]["implementation"] =
            serde_json::to_value(id::<OracleStrategyImplementationArtifact>(
                "other implementation",
            ))
            .expect("implementation json");
        let drifted_run: OracleStrategyRunV1 =
            serde_json::from_value(drifted_run_json).expect("structurally valid drifted run");
        assert!(matches!(
            ledger.start_strategy(&drifted_run, &catalog, budget),
            Err(OracleFrameworkError::StrategyRunBindingMismatch)
        ));

        let running = ledger
            .start_strategy(&run, &catalog, budget)
            .expect("start");
        let wrong_tools_request = OracleExperimentRequestV1::new(
            item,
            run_id,
            id("other experiment tools"),
            OracleExperimentOperationName::new("execute-probe").expect("operation"),
            id("arguments"),
        );
        assert!(matches!(
            running.request_experiment(&wrong_tools_request, &workspace),
            Err(OracleFrameworkError::ExperimentBindingMismatch)
        ));
        let proposed = running
            .request_experiment(&request, &workspace)
            .expect("request");
        let authorized = proposed
            .authorize_experiment(&request, budget)
            .expect("authorize");
        let observation =
            OracleExplorationObservationV1::worker_experiment(&request, &receipt, id("payload"))
                .expect("worker observation");
        let mismatched_receipt = TrustedOracleWorkerReceiptV1::new(
            id("other request"),
            id("job contract"),
            id("execution receipt"),
        );
        assert!(matches!(
            OracleExplorationObservationV1::worker_experiment(
                &request,
                &mismatched_receipt,
                id("payload"),
            ),
            Err(OracleFrameworkError::ExperimentBindingMismatch)
        ));
        let observation_id = observation.identity().expect("observation");
        let resumed = authorized
            .record_experiment_observation(&request, &receipt, &observation)
            .expect("observation projection");
        let element = OraclePortfolioElementV1::new(
            item,
            run_id,
            OraclePortfolioElementKindV1::Reference(id("reference")),
            vec![observation_id],
        )
        .expect("element");
        let contributed = resumed
            .record_contribution(item, run_id, &[element])
            .expect("contribution");

        assert_eq!(contributed.revision().get(), 6);
        assert!(contributed.parent.is_some());
        assert!(matches!(
            contributed.entries()[0].resolution,
            OracleObligationResolutionV1::Contributed { .. }
        ));

        let wrong_request = OracleExperimentRequestV1::new(
            item,
            run_id,
            experiment_tools,
            OracleExperimentOperationName::new("execute-probe").expect("operation"),
            id("other arguments"),
        );
        let wrong_receipt = TrustedOracleWorkerReceiptV1::new(
            wrong_request.identity().expect("request"),
            id("job contract"),
            id("execution receipt"),
        );
        let wrong = OracleExplorationObservationV1::worker_experiment(
            &wrong_request,
            &wrong_receipt,
            id("payload"),
        )
        .expect("wrong observation");
        assert!(matches!(
            authorized.record_experiment_observation(&request, &receipt, &wrong),
            Err(OracleFrameworkError::ObservationBindingMismatch)
        ));
    }

    #[test]
    fn persisted_work_item_cannot_move_a_concern_to_another_plane() {
        let item = OracleWorkItemV1::new(
            claim("claim-a"),
            OracleConcernV1::ObservableOutputs,
            OracleStrategyRoleV1::Synthesis,
        );
        let mut json = serde_json::to_value(item).expect("json");
        json["plane"] = serde_json::json!("input-domain");
        assert!(serde_json::from_value::<OracleWorkItemV1>(json).is_err());
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum SkeletonStep {
        Workspace,
        Claims,
        WorkItems,
        Open,
        Select,
        Strategy,
        Experiment,
        Portfolio,
    }

    #[derive(Default)]
    struct RecordedExplorationStages {
        trace: Vec<SkeletonStep>,
    }

    impl OracleExplorationStages for RecordedExplorationStages {
        type Error = ();
        type AdmittedIntent = ();
        type Workspace = ();
        type Claims = ();
        type WorkItems = ();
        type Exploration = u8;
        type StrategyAction = ();
        type ExperimentAction = ();
        type Waiting = ();
        type Portfolio = ();

        fn freeze_oracle_workspace(
            &mut self,
            _intent: &Self::AdmittedIntent,
        ) -> impl Future<Output = Result<Self::Workspace, Self::Error>> + Send {
            self.trace.push(SkeletonStep::Workspace);
            ready(Ok(()))
        }

        fn derive_oracle_claims(
            &mut self,
            _workspace: &Self::Workspace,
            _intent: &Self::AdmittedIntent,
        ) -> Result<Self::Claims, Self::Error> {
            self.trace.push(SkeletonStep::Claims);
            Ok(())
        }

        fn derive_oracle_work_items(
            &mut self,
            _workspace: &Self::Workspace,
            _claims: &Self::Claims,
        ) -> Result<Self::WorkItems, Self::Error> {
            self.trace.push(SkeletonStep::WorkItems);
            Ok(())
        }

        fn open_or_recover_oracle_exploration(
            &mut self,
            _workspace: &Self::Workspace,
            _work_items: &Self::WorkItems,
        ) -> Result<Self::Exploration, Self::Error> {
            self.trace.push(SkeletonStep::Open);
            Ok(0)
        }

        fn select_next_oracle_action(
            &mut self,
            exploration: &Self::Exploration,
        ) -> Result<
            OracleExplorationDirectiveV1<
                Self::StrategyAction,
                Self::ExperimentAction,
                Self::Waiting,
            >,
            Self::Error,
        > {
            self.trace.push(SkeletonStep::Select);
            Ok(match exploration {
                0 => OracleExplorationDirectiveV1::RunStrategy(()),
                1 => OracleExplorationDirectiveV1::AuthorizeControllerExperiment(()),
                _ => OracleExplorationDirectiveV1::FreezePortfolio,
            })
        }

        fn run_oracle_strategy(
            &mut self,
            _workspace: &Self::Workspace,
            _exploration: Self::Exploration,
            _action: Self::StrategyAction,
        ) -> impl Future<Output = Result<Self::Exploration, Self::Error>> + Send {
            self.trace.push(SkeletonStep::Strategy);
            ready(Ok(1))
        }

        fn authorize_and_run_oracle_experiment(
            &mut self,
            _workspace: &Self::Workspace,
            _exploration: Self::Exploration,
            _action: Self::ExperimentAction,
        ) -> impl Future<Output = Result<Self::Exploration, Self::Error>> + Send {
            self.trace.push(SkeletonStep::Experiment);
            ready(Ok(2))
        }

        fn freeze_oracle_portfolio(
            &mut self,
            _workspace: Self::Workspace,
            _exploration: Self::Exploration,
        ) -> Result<Self::Portfolio, Self::Error> {
            self.trace.push(SkeletonStep::Portfolio);
            Ok(())
        }
    }

    #[tokio::test]
    async fn readable_exploration_skeleton_keeps_controller_authority_in_the_loop() {
        let mut stages = RecordedExplorationStages::default();
        let outcome = super::run_oracle_exploration(&mut stages, &())
            .await
            .expect("workflow");
        assert!(matches!(
            outcome,
            OracleExplorationRunOutcomeV1::Portfolio(())
        ));
        assert_eq!(
            stages.trace,
            vec![
                SkeletonStep::Workspace,
                SkeletonStep::Claims,
                SkeletonStep::WorkItems,
                SkeletonStep::Open,
                SkeletonStep::Select,
                SkeletonStep::Strategy,
                SkeletonStep::Select,
                SkeletonStep::Experiment,
                SkeletonStep::Select,
                SkeletonStep::Portfolio,
            ]
        );
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one linear control compares partial, rejected, and Candidate-admitted projections"
    )]
    fn admission_missing_receipts_is_partial_and_failed_control_is_rejected() {
        let claim_body = OracleClaimV1::new(
            TaskId::new(),
            id("admitted intent"),
            OracleClaimName::new("transform-output").expect("claim name"),
            AuthoritativeIntentClaimV1::new(
                crate::OperationIntentV1::new(
                    vec![
                        crate::SirCallerClaimV1::new(
                            crate::SirCallerClaimId::new("selected-output").expect("caller claim"),
                            crate::SirIntentLayer::ObservableContract,
                            crate::SirCallerClaimStatement::new(
                                "preserve observable transform outputs",
                            )
                            .expect("caller claim statement"),
                            vec![],
                        )
                        .expect("caller claim"),
                    ],
                    crate::SirIntentLayer::ObservableContract,
                    crate::SirHypothesisClaim::new("preserve observable transform outputs")
                        .expect("semantics"),
                    crate::SirIntentDomain::new("all caller-authorized inputs").expect("domain"),
                )
                .expect("operation intent"),
            ),
        );
        let item = OracleWorkItemV1::new(
            claim_body.identity().expect("claim id"),
            OracleConcernV1::ObservableOutputs,
            OracleStrategyRoleV1::Synthesis,
        );
        let material_bytes = b"task-generic admitted reference semantics".to_vec();
        let reference =
            ContentId::<ReferenceArtifact>::derive(&material_bytes).expect("reference identity");
        let element = OraclePortfolioElementV1::new(
            item.identity().expect("item id"),
            id("run"),
            OraclePortfolioElementKindV1::Reference(reference),
            vec![],
        )
        .expect("portfolio element");
        let entry = OracleObligationEntryV1 {
            item: item.clone(),
            resolution: OracleObligationResolutionV1::Contributed {
                run: id("run"),
                elements: vec![element.identity().expect("element id")],
                observations: vec![],
            },
        };
        let ledger = OracleExplorationLedgerV1 {
            schema_version: SCHEMA_V1,
            workspace: id("workspace"),
            parent: None,
            revision: OracleExplorationRevision::new(1).expect("revision"),
            entries: vec![entry],
            strategy_runs_started: 1,
            experiments_started: 0,
        };
        let proposal = OraclePortfolioProposalV1::freeze(&ledger).expect("proposal");
        let (policy, mechanisms, attempt) = admission_context(&proposal);
        let no_evidence =
            OracleAdmissionEvidenceV1::new(&attempt, Vec::new()).expect("empty evidence");
        let partial =
            recompute_oracle_admission(&proposal, &policy, &mechanisms, &attempt, &no_evidence)
                .expect("partial");
        assert_eq!(
            partial.claims[0].status,
            OracleClaimAdmissionStatusV1::Partial
        );
        let mut forged = serde_json::to_value(&partial).expect("outcome json");
        forged["claims"][0]["status"] = serde_json::json!("admitted");
        assert!(serde_json::from_value::<OracleAdmissionOutcomeV1>(forged).is_err());

        let unknown_item_receipt = OracleControlReceiptV1::new(
            proposal.identity().expect("proposal id"),
            id("unknown-item"),
            OracleControlFamilyV1::Honest,
            mechanisms
                .mechanism(OracleControlFamilyV1::Honest)
                .expect("honest mechanism"),
            id("unknown-item-receipt"),
            OracleControlResultV1::Passed,
        );
        assert!(matches!(
            OracleAdmissionEvidenceV1::new(&attempt, vec![unknown_item_receipt]),
            Err(OracleFrameworkError::AdmissionEvidenceBindingMismatch)
        ));

        let reused_receipt = id("reused trusted receipt");
        let duplicate_provenance = vec![
            OracleControlReceiptV1::new(
                proposal.identity().expect("proposal id"),
                item.identity().expect("item id"),
                OracleControlFamilyV1::Honest,
                mechanisms
                    .mechanism(OracleControlFamilyV1::Honest)
                    .expect("honest mechanism"),
                reused_receipt,
                OracleControlResultV1::Passed,
            ),
            OracleControlReceiptV1::new(
                proposal.identity().expect("proposal id"),
                item.identity().expect("item id"),
                OracleControlFamilyV1::Mutant,
                mechanisms
                    .mechanism(OracleControlFamilyV1::Mutant)
                    .expect("mutant mechanism"),
                reused_receipt,
                OracleControlResultV1::Passed,
            ),
        ];
        assert!(matches!(
            OracleAdmissionEvidenceV1::new(&attempt, duplicate_provenance),
            Err(OracleFrameworkError::DuplicateControlReceipt)
        ));

        let failed = OracleControlReceiptV1::new(
            proposal.identity().expect("proposal id"),
            item.identity().expect("item id"),
            OracleControlFamilyV1::Mutant,
            mechanisms
                .mechanism(OracleControlFamilyV1::Mutant)
                .expect("mutant mechanism"),
            id("receipt"),
            OracleControlResultV1::Failed,
        );
        let failed_evidence =
            OracleAdmissionEvidenceV1::new(&attempt, vec![failed]).expect("failed evidence");
        let rejected =
            recompute_oracle_admission(&proposal, &policy, &mechanisms, &attempt, &failed_evidence)
                .expect("rejected");
        assert_eq!(
            rejected.claims[0].status,
            OracleClaimAdmissionStatusV1::Rejected
        );

        assert!(matches!(
            crate::CandidateOracleContractV1::derive(&proposal, &partial),
            Err(crate::CandidateExplorationError::NoAdmittedOracleClaims)
        ));
        let admitted_receipts = policy
            .required_controls()
            .iter()
            .map(|control| {
                OracleControlReceiptV1::new(
                    proposal.identity().expect("proposal id"),
                    item.identity().expect("item id"),
                    *control,
                    mechanisms.mechanism(*control).expect("qualified mechanism"),
                    id(match control {
                        OracleControlFamilyV1::MechanismQualification => "qualified receipt",
                        OracleControlFamilyV1::Honest => "honest receipt",
                        OracleControlFamilyV1::Mutant => "mutant receipt",
                        OracleControlFamilyV1::Hidden => "hidden receipt",
                        OracleControlFamilyV1::Bypass => "bypass receipt",
                    }),
                    OracleControlResultV1::Passed,
                )
            })
            .collect();
        let admitted_evidence =
            OracleAdmissionEvidenceV1::new(&attempt, admitted_receipts).expect("admitted evidence");
        let admitted = recompute_oracle_admission(
            &proposal,
            &policy,
            &mechanisms,
            &attempt,
            &admitted_evidence,
        )
        .expect("admitted outcome");
        let candidate_contract = crate::CandidateOracleContractV1::derive(&proposal, &admitted)
            .expect("Candidate Oracle contract");
        assert_eq!(
            candidate_contract.outcome(),
            admitted.identity().expect("outcome id")
        );
        assert_eq!(
            candidate_contract.admitted_claims()[0].claim(),
            item.claim()
        );
        let material =
            crate::CandidateOracleMaterialV1::from_portfolio_kind(element.kind(), material_bytes)
                .expect("typed Oracle material");
        let candidate_materials = crate::CandidateOracleMaterialsV1::new(
            &candidate_contract,
            vec![claim_body],
            vec![
                crate::CandidateOracleElementMaterialV1::new(element, material)
                    .expect("element body"),
            ],
        )
        .expect("complete Candidate-visible Oracle material");
        candidate_materials
            .validate_against(&candidate_contract)
            .expect("exact admitted material set");
        let mut drifted = serde_json::to_value(&candidate_materials).expect("materials json");
        drifted["elements"][0]["material"]["bytes"][0] = serde_json::json!(0);
        assert!(serde_json::from_value::<crate::CandidateOracleMaterialsV1>(drifted).is_err());
    }

    #[test]
    fn admission_never_promotes_a_coverage_gap_even_when_every_control_passes() {
        let item = OracleWorkItemV1::new(
            claim("claim-a"),
            OracleConcernV1::ObservableOutputs,
            OracleStrategyRoleV1::Synthesis,
        );
        let item_id = item.identity().expect("item id");
        let ledger = OracleExplorationLedgerV1 {
            schema_version: SCHEMA_V1,
            workspace: id("workspace"),
            parent: None,
            revision: OracleExplorationRevision::new(1).expect("revision"),
            entries: vec![OracleObligationEntryV1 {
                item,
                resolution: OracleObligationResolutionV1::CoverageGap {
                    run: id("run"),
                    elements: vec![id("gap element")],
                    observations: vec![],
                },
            }],
            strategy_runs_started: 1,
            experiments_started: 0,
        };
        let proposal = OraclePortfolioProposalV1::freeze(&ledger).expect("proposal");
        let proposal_id = proposal.identity().expect("proposal id");
        let (policy, mechanisms, attempt) = admission_context(&proposal);
        let receipts = policy
            .required_controls()
            .iter()
            .map(|control| {
                OracleControlReceiptV1::new(
                    proposal_id,
                    item_id,
                    *control,
                    mechanisms.mechanism(*control).expect("qualified mechanism"),
                    id(match control {
                        OracleControlFamilyV1::MechanismQualification => {
                            "qualification trusted receipt"
                        }
                        OracleControlFamilyV1::Honest => "honest trusted receipt",
                        OracleControlFamilyV1::Mutant => "mutant trusted receipt",
                        OracleControlFamilyV1::Hidden => "hidden trusted receipt",
                        OracleControlFamilyV1::Bypass => "bypass trusted receipt",
                    }),
                    OracleControlResultV1::Passed,
                )
            })
            .collect::<Vec<_>>();
        let evidence =
            OracleAdmissionEvidenceV1::new(&attempt, receipts).expect("control evidence");

        let outcome =
            recompute_oracle_admission(&proposal, &policy, &mechanisms, &attempt, &evidence)
                .expect("admission");
        assert_eq!(
            outcome.claims[0].status,
            OracleClaimAdmissionStatusV1::Partial
        );
        assert_eq!(outcome.claims[0].unresolved_items, vec![item_id]);
    }
}
