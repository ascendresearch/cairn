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

use cairn_execution::{
    ExecutionReceiptArtifact, ExecutionStderrArtifact, ExecutionStdoutArtifact, JobContractArtifact,
};
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
artifact!(OracleDimensionArtifact, "migration.oracle-dimension.v1");
artifact!(OracleItemArtifact, "migration.oracle-item.v1");
artifact!(
    OracleDimensionItemSetProposalArtifact,
    "migration.oracle-dimension-item-set-proposal.v1"
);
artifact!(
    OracleDimensionItemSetReviewArtifact,
    "migration.oracle-dimension-item-set-review.v1"
);
artifact!(OracleItemDraftArtifact, "migration.oracle-item-draft.v1");
artifact!(OracleItemReviewArtifact, "migration.oracle-item-review.v1");
artifact!(
    OraclePortfolioCoherenceReviewArtifact,
    "migration.oracle-portfolio-coherence-review.v1"
);
artifact!(
    OracleCoherentPortfolioArtifact,
    "migration.oracle-coherent-portfolio.v1"
);
artifact!(
    OracleAcceptedItemArtifact,
    "migration.oracle-accepted-item.v1"
);
artifact!(
    OracleWholePortfolioProposalAuthorityArtifact,
    "migration.oracle-whole-portfolio-proposal-authority.v1"
);
artifact!(OracleCheckPlanArtifact, "migration.oracle-check-plan.v1");
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
    OracleRevisionRequestArtifact,
    "migration.oracle-revision-request.v1"
);
artifact!(
    OracleControlReconciliationRequestArtifact,
    "migration.oracle-control-reconciliation-request.v1"
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

macro_rules! bounded_text {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, PartialEq, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, OracleFrameworkError> {
                let value = value.into();
                if value.trim() != value || value.is_empty() || value.len() > 4_096 {
                    return Err(OracleFrameworkError::InvalidText($kind));
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

bounded_text!(OracleCheckObjective, "Oracle check objective");
bounded_text!(OracleItemStatement, "Oracle item statement");
bounded_text!(OracleCheckSetup, "Oracle check setup");
bounded_text!(OracleCheckObservation, "Oracle check observation");
bounded_text!(OracleCheckPassCondition, "Oracle check pass condition");
bounded_text!(OracleReviewExplanation, "Oracle review explanation");
bounded_text!(OracleReviewRequiredChange, "Oracle review required change");
bounded_text!(
    OracleControlDiagnosticSummary,
    "Oracle control diagnostic summary"
);

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
/// Every admitted claim remains bound to the complete contract identity; its planes and concerns
/// are expanded separately by [`derive_oracle_dimensions`]. Callers cannot supply or remove
/// claims.
#[must_use]
pub fn derive_oracle_claims(
    task_id: TaskId,
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    admitted_claims: &[AuthoritativeIntentClaimV1],
) -> Vec<OracleClaimV1> {
    admitted_claims
        .iter()
        .enumerate()
        .map(|(index, claim)| {
            OracleClaimV1::new(
                task_id,
                admitted_intent,
                OracleClaimName(format!("admitted-intent-{index:04}")),
                claim.clone(),
            )
        })
        .collect()
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
    ReadItemConversation,
    SubmitItemDraft,
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
                OracleStrategyToolV1::ReadItemConversation,
                OracleStrategyToolV1::SubmitItemDraft,
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

    fn supports(&self, dimension: &OracleDimensionV1) -> bool {
        self.roles.contains(&dimension.role) && self.concerns.contains(&dimension.concern)
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

    fn eligible(&self, dimension: &OracleDimensionV1) -> Vec<OracleStrategyName> {
        self.strategies
            .iter()
            .filter(|strategy| strategy.supports(dimension))
            .map(|strategy| strategy.name.clone())
            .collect()
    }

    fn resolve(
        &self,
        dimension: &OracleDimensionV1,
        strategy: &OracleStrategyName,
    ) -> Result<&OracleStrategyRegistrationV1, OracleFrameworkError> {
        self.strategies
            .iter()
            .find(|registration| registration.name == *strategy && registration.supports(dimension))
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

/// Monotonic immutable revision inside one exact Oracle item conversation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OracleItemRevision(u32);

impl OracleItemRevision {
    pub const fn new(value: u32) -> Result<Self, OracleFrameworkError> {
        if value == 0 {
            Err(OracleFrameworkError::NonPositive("Oracle item revision"))
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

impl<'de> Deserialize<'de> for OracleItemRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Maximum revision number authorized for each exact Oracle item conversation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OracleItemRevisionLimit(u32);

impl OracleItemRevisionLimit {
    pub const fn new(value: u32) -> Result<Self, OracleFrameworkError> {
        if value == 0 {
            Err(OracleFrameworkError::NonPositive(
                "Oracle item revision limit",
            ))
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Maximum revision number authorized for one exact dimension item-discovery conversation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OracleItemDiscoveryRevisionLimit(u32);

impl OracleItemDiscoveryRevisionLimit {
    pub const fn new(value: u32) -> Result<Self, OracleFrameworkError> {
        if value == 0 {
            Err(OracleFrameworkError::NonPositive(
                "Oracle item discovery revision limit",
            ))
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for OracleItemDiscoveryRevisionLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for OracleItemRevisionLimit {
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
    pub item_discovery_revisions: OracleItemDiscoveryRevisionLimit,
    pub item_revisions: OracleItemRevisionLimit,
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
/// A claim identity cannot be substituted where a fully scoped dimension identity is required.
///
/// ```compile_fail
/// use cairn_migration::{OracleClaimArtifact, OracleDimensionArtifact};
/// use cairn_protocol::ContentId;
/// fn require_dimension(_: ContentId<OracleDimensionArtifact>) {}
/// fn wrong(claim: ContentId<OracleClaimArtifact>) { require_dimension(claim); }
/// ```
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct OracleDimensionV1 {
    claim: ContentId<OracleClaimArtifact>,
    plane: OraclePlaneV1,
    concern: OracleConcernV1,
    role: OracleStrategyRoleV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleDimensionWire {
    claim: ContentId<OracleClaimArtifact>,
    plane: OraclePlaneV1,
    concern: OracleConcernV1,
    role: OracleStrategyRoleV1,
}

impl Ord for OracleDimensionV1 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.claim
            .to_wire()
            .cmp(&other.claim.to_wire())
            .then_with(|| self.plane.cmp(&other.plane))
            .then_with(|| self.concern.cmp(&other.concern))
            .then_with(|| self.role.cmp(&other.role))
    }
}

impl TryFrom<OracleDimensionWire> for OracleDimensionV1 {
    type Error = OracleFrameworkError;
    fn try_from(wire: OracleDimensionWire) -> Result<Self, Self::Error> {
        if wire.plane != wire.concern.plane() {
            return Err(OracleFrameworkError::DimensionPlaneMismatch);
        }
        Ok(Self::new(wire.claim, wire.concern, wire.role))
    }
}

impl<'de> Deserialize<'de> for OracleDimensionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleDimensionWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl PartialOrd for OracleDimensionV1 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl OracleDimensionV1 {
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
    pub fn identity(&self) -> Result<ContentId<OracleDimensionArtifact>, OracleFrameworkError> {
        derive_id(self)
    }
}

/// One independently generated and reviewed Oracle obligation inside an exact Controller-derived
/// dimension. A dimension may contain any positive number of items; the model supplies the
/// statement, while the gateway binds it to the offered dimension and derives its identity.
///
/// ```compile_fail
/// use cairn_migration::{OracleDimensionArtifact, OracleItemArtifact};
/// use cairn_protocol::ContentId;
/// fn require_item(_: ContentId<OracleItemArtifact>) {}
/// fn wrong(dimension: ContentId<OracleDimensionArtifact>) { require_item(dimension); }
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleItemV1 {
    schema_version: u16,
    dimension: ContentId<OracleDimensionArtifact>,
    statement: OracleItemStatement,
}

impl Ord for OracleItemV1 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.dimension
            .to_wire()
            .cmp(&other.dimension.to_wire())
            .then_with(|| self.statement.as_str().cmp(other.statement.as_str()))
    }
}

impl PartialOrd for OracleItemV1 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleItemWire {
    schema_version: u16,
    dimension: ContentId<OracleDimensionArtifact>,
    statement: OracleItemStatement,
}

impl OracleItemV1 {
    pub fn new(
        dimension: ContentId<OracleDimensionArtifact>,
        statement: OracleItemStatement,
    ) -> Result<Self, OracleFrameworkError> {
        Ok(Self {
            schema_version: SCHEMA_V1,
            dimension,
            statement,
        })
    }

    #[must_use]
    pub const fn dimension(&self) -> ContentId<OracleDimensionArtifact> {
        self.dimension
    }

    #[must_use]
    pub const fn statement(&self) -> &OracleItemStatement {
        &self.statement
    }

    pub fn identity(&self) -> Result<ContentId<OracleItemArtifact>, OracleFrameworkError> {
        derive_id(self)
    }
}

impl TryFrom<OracleItemWire> for OracleItemV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleItemWire) -> Result<Self, Self::Error> {
        require_v1(wire.schema_version)?;
        Self::new(wire.dimension, wire.statement)
    }
}

impl<'de> Deserialize<'de> for OracleItemV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleItemWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct OracleItemSetRevision(u32);

impl OracleItemSetRevision {
    pub const fn new(value: u32) -> Result<Self, OracleFrameworkError> {
        if value == 0 {
            Err(OracleFrameworkError::NonPositive(
                "Oracle item set revision",
            ))
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

impl<'de> Deserialize<'de> for OracleItemSetRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Non-authoritative item decomposition for one exact Controller-derived dimension. It contains
/// no plans, Review decision, receipt, or Admission authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleDimensionItemSetProposalV1 {
    schema_version: u16,
    dimension: ContentId<OracleDimensionArtifact>,
    parent: Option<ContentId<OracleDimensionItemSetProposalArtifact>>,
    revision: OracleItemSetRevision,
    items: Vec<OracleItemV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleDimensionItemSetProposalWire {
    schema_version: u16,
    dimension: ContentId<OracleDimensionArtifact>,
    parent: Option<ContentId<OracleDimensionItemSetProposalArtifact>>,
    revision: OracleItemSetRevision,
    items: Vec<OracleItemV1>,
}

impl OracleDimensionItemSetProposalV1 {
    pub fn new(
        dimension: ContentId<OracleDimensionArtifact>,
        mut items: Vec<OracleItemV1>,
    ) -> Result<Self, OracleFrameworkError> {
        items.sort();
        let value = Self {
            schema_version: SCHEMA_V1,
            dimension,
            parent: None,
            revision: OracleItemSetRevision::new(1)?,
            items,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn revise(
        previous: &Self,
        mut items: Vec<OracleItemV1>,
    ) -> Result<Self, OracleFrameworkError> {
        items.sort();
        let value = Self {
            schema_version: SCHEMA_V1,
            dimension: previous.dimension,
            parent: Some(previous.identity()?),
            revision: previous.revision.next()?,
            items,
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub const fn dimension(&self) -> ContentId<OracleDimensionArtifact> {
        self.dimension
    }

    #[must_use]
    pub fn items(&self) -> &[OracleItemV1] {
        &self.items
    }

    #[must_use]
    pub const fn parent(&self) -> Option<ContentId<OracleDimensionItemSetProposalArtifact>> {
        self.parent
    }

    #[must_use]
    pub const fn revision(&self) -> OracleItemSetRevision {
        self.revision
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleDimensionItemSetProposalArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        validate_strict(&self.items, "Oracle dimension item set")?;
        if self
            .items
            .iter()
            .any(|item| item.dimension() != self.dimension)
        {
            return Err(OracleFrameworkError::OracleItemBindingMismatch);
        }
        if (self.revision.get() == 1) != self.parent.is_none() {
            return Err(OracleFrameworkError::RevisionLineageMismatch);
        }
        Ok(())
    }
}

impl TryFrom<OracleDimensionItemSetProposalWire> for OracleDimensionItemSetProposalV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleDimensionItemSetProposalWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            dimension: wire.dimension,
            parent: wire.parent,
            revision: wire.revision,
            items: wire.items,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleDimensionItemSetProposalV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleDimensionItemSetProposalWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleItemSetReviewIssueClassV1 {
    IncompleteCoverage,
    OverlappingItems,
    VagueItem,
    OutOfDimension,
    NotCandidateFacing,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleItemSetReviewFindingV1 {
    issue: OracleItemSetReviewIssueClassV1,
    explanation: OracleReviewExplanation,
    required_change: OracleReviewRequiredChange,
}

impl OracleItemSetReviewFindingV1 {
    #[must_use]
    pub fn new(
        issue: OracleItemSetReviewIssueClassV1,
        explanation: OracleReviewExplanation,
        required_change: OracleReviewRequiredChange,
    ) -> Self {
        Self {
            issue,
            explanation,
            required_change,
        }
    }

    #[must_use]
    pub const fn issue(&self) -> OracleItemSetReviewIssueClassV1 {
        self.issue
    }

    #[must_use]
    pub const fn explanation(&self) -> &OracleReviewExplanation {
        &self.explanation
    }

    #[must_use]
    pub const fn required_change(&self) -> &OracleReviewRequiredChange {
        &self.required_change
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "decision", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OracleDimensionItemSetReviewDecisionV1 {
    Approved,
    NeedsRevision {
        findings: Vec<OracleItemSetReviewFindingV1>,
    },
}

/// Independent semantic Review of one exact dimension item-decomposition revision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleDimensionItemSetReviewV1 {
    schema_version: u16,
    dimension: ContentId<OracleDimensionArtifact>,
    proposal: ContentId<OracleDimensionItemSetProposalArtifact>,
    decision: OracleDimensionItemSetReviewDecisionV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleDimensionItemSetReviewWire {
    schema_version: u16,
    dimension: ContentId<OracleDimensionArtifact>,
    proposal: ContentId<OracleDimensionItemSetProposalArtifact>,
    decision: OracleDimensionItemSetReviewDecisionV1,
}

impl OracleDimensionItemSetReviewV1 {
    pub fn approved(
        proposal: &OracleDimensionItemSetProposalV1,
    ) -> Result<Self, OracleFrameworkError> {
        Ok(Self {
            schema_version: SCHEMA_V1,
            dimension: proposal.dimension(),
            proposal: proposal.identity()?,
            decision: OracleDimensionItemSetReviewDecisionV1::Approved,
        })
    }

    pub fn needs_revision(
        proposal: &OracleDimensionItemSetProposalV1,
        findings: Vec<OracleItemSetReviewFindingV1>,
    ) -> Result<Self, OracleFrameworkError> {
        let mut encoded = findings
            .into_iter()
            .map(|finding| {
                cairn_codec::to_vec(&finding)
                    .map(|bytes| (bytes, finding))
                    .map_err(|error| OracleFrameworkError::Codec(error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        encoded.sort_by(|left, right| left.0.cmp(&right.0));
        if encoded.is_empty() || encoded.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(OracleFrameworkError::ReviewFindingsInvalid);
        }
        let value = Self {
            schema_version: SCHEMA_V1,
            dimension: proposal.dimension(),
            proposal: proposal.identity()?,
            decision: OracleDimensionItemSetReviewDecisionV1::NeedsRevision {
                findings: encoded.into_iter().map(|(_, finding)| finding).collect(),
            },
        };
        value.validate_against(proposal)?;
        Ok(value)
    }

    #[must_use]
    pub const fn dimension(&self) -> ContentId<OracleDimensionArtifact> {
        self.dimension
    }

    #[must_use]
    pub const fn proposal(&self) -> ContentId<OracleDimensionItemSetProposalArtifact> {
        self.proposal
    }

    #[must_use]
    pub const fn decision(&self) -> &OracleDimensionItemSetReviewDecisionV1 {
        &self.decision
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleDimensionItemSetReviewArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    pub fn validate_against(
        &self,
        proposal: &OracleDimensionItemSetProposalV1,
    ) -> Result<(), OracleFrameworkError> {
        self.validate()?;
        if self.dimension != proposal.dimension() || self.proposal != proposal.identity()? {
            return Err(OracleFrameworkError::ReviewProposalMismatch);
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        if let OracleDimensionItemSetReviewDecisionV1::NeedsRevision { findings } = &self.decision {
            if findings.is_empty() {
                return Err(OracleFrameworkError::ReviewFindingsInvalid);
            }
            let encoded = findings
                .iter()
                .map(cairn_codec::to_vec)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| OracleFrameworkError::Codec(error.to_string()))?;
            if encoded.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(OracleFrameworkError::ReviewFindingsInvalid);
            }
        }
        Ok(())
    }
}

impl TryFrom<OracleDimensionItemSetReviewWire> for OracleDimensionItemSetReviewV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleDimensionItemSetReviewWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            dimension: wire.dimension,
            proposal: wire.proposal,
            decision: wire.decision,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleDimensionItemSetReviewV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleDimensionItemSetReviewWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Method selected by a proposal strategy for one exact Oracle coverage obligation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleCheckMethodV1 {
    StaticAnalysis,
    ReferenceExecution,
    Metamorphic,
    BoundaryProbe,
    RuntimeObservation,
}

/// Exact evidence edge supporting one proposed Oracle check.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "source", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OracleCheckEvidenceV1 {
    SourceCitation {
        citation: crate::SirSourceCitationV1,
    },
    AdmittedIntent {
        contract: ContentId<MigrationIntentContractArtifact>,
    },
}

/// Exact binary32 allowance, carried as its bit pattern so JSON cannot round it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct OracleAllowanceBitsV1(u32);

impl OracleAllowanceBitsV1 {
    /// Creates one exact binary32 allowance from its bit pattern.
    #[must_use]
    pub const fn new(bits: u32) -> Self {
        Self(bits)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Returns whether this allowance is a finite, non-negative binary32 value.
    ///
    /// A negative or non-finite allowance accepts everything or nothing, and either way it is not
    /// a tolerance.
    #[must_use]
    pub fn is_usable(self) -> bool {
        let value = f32::from_bits(self.0);
        value.is_finite() && value >= 0.0
    }
}

/// How a candidate observation is compared with what the check requires.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum OracleComparatorV1 {
    /// The observed bytes have to match exactly.
    ExactBytes,
    /// Elementwise binary32 comparison inside an absolute allowance.
    AbsoluteBinary32 { allowance: OracleAllowanceBitsV1 },
    /// Elementwise binary32 comparison inside a relative allowance.
    RelativeBinary32 { allowance: OracleAllowanceBitsV1 },
}

/// Where a numerical allowance came from.
///
/// An allowance with no stated origin is a number somebody chose, and a judge resting on one
/// cannot say how wrong a candidate would have to be before it complained.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleAllowanceProvenanceV1 {
    /// The caller declared the tolerance as part of the desired contract.
    CallerDeclared,
    /// Measured as the difference between independent runs of a reference.
    MeasuredNoiseFloor,
    /// Derived from the arithmetic the operation performs, and stated in the plan's setup.
    DerivedFromArithmetic,
    /// No allowance is claimed, because the comparison is exact.
    NotApplicable,
}

/// The machine-evaluable half of a check plan's pass condition.
///
/// The prose pass condition says what a reader should conclude; this says what a runner should
/// compute. Without it a mechanism can only confirm that the prose is non-empty, which is a fact
/// about the plan and not about any candidate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleCheckAssertionV1 {
    comparator: OracleComparatorV1,
    allowance_provenance: OracleAllowanceProvenanceV1,
}

impl OracleCheckAssertionV1 {
    /// Binds one comparator to the origin of the allowance it uses.
    ///
    /// # Errors
    ///
    /// Rejects a tolerant comparator whose allowance is unusable or has no stated origin, and an
    /// exact comparator that claims an origin it does not need.
    pub fn new(
        comparator: OracleComparatorV1,
        allowance_provenance: OracleAllowanceProvenanceV1,
    ) -> Result<Self, OracleFrameworkError> {
        let value = Self {
            comparator,
            allowance_provenance,
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub const fn comparator(&self) -> OracleComparatorV1 {
        self.comparator
    }

    #[must_use]
    pub const fn allowance_provenance(&self) -> OracleAllowanceProvenanceV1 {
        self.allowance_provenance
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        let allowance = match self.comparator {
            OracleComparatorV1::ExactBytes => {
                return if matches!(
                    self.allowance_provenance,
                    OracleAllowanceProvenanceV1::NotApplicable
                ) {
                    Ok(())
                } else {
                    Err(OracleFrameworkError::UnjustifiedAllowance)
                };
            }
            OracleComparatorV1::AbsoluteBinary32 { allowance }
            | OracleComparatorV1::RelativeBinary32 { allowance } => allowance,
        };
        if !allowance.is_usable()
            || matches!(
                self.allowance_provenance,
                OracleAllowanceProvenanceV1::NotApplicable
            )
        {
            return Err(OracleFrameworkError::UnjustifiedAllowance);
        }
        Ok(())
    }
}

/// Model-proposed, non-authoritative check plan bound to one exact Oracle item.
///
/// The plan can describe how evidence should be obtained, but it cannot create an observation,
/// a qualified-control receipt, or an Admission result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleCheckPlanV1 {
    schema_version: u16,
    item: ContentId<OracleItemArtifact>,
    method: OracleCheckMethodV1,
    objective: OracleCheckObjective,
    setup: OracleCheckSetup,
    observation: OracleCheckObservation,
    pass_condition: OracleCheckPassCondition,
    assertion: OracleCheckAssertionV1,
    evidence: Vec<OracleCheckEvidenceV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleCheckPlanWire {
    schema_version: u16,
    item: ContentId<OracleItemArtifact>,
    method: OracleCheckMethodV1,
    objective: OracleCheckObjective,
    setup: OracleCheckSetup,
    observation: OracleCheckObservation,
    pass_condition: OracleCheckPassCondition,
    assertion: OracleCheckAssertionV1,
    evidence: Vec<OracleCheckEvidenceV1>,
}

impl OracleCheckPlanV1 {
    #[allow(
        clippy::too_many_arguments,
        reason = "a check plan keeps every independently authored field explicit at its constructor"
    )]
    pub fn new(
        item: ContentId<OracleItemArtifact>,
        method: OracleCheckMethodV1,
        objective: OracleCheckObjective,
        setup: OracleCheckSetup,
        observation: OracleCheckObservation,
        pass_condition: OracleCheckPassCondition,
        assertion: OracleCheckAssertionV1,
        evidence: Vec<OracleCheckEvidenceV1>,
    ) -> Result<Self, OracleFrameworkError> {
        let value = Self {
            schema_version: SCHEMA_V1,
            item,
            method,
            objective,
            setup,
            observation,
            pass_condition,
            assertion,
            evidence,
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub const fn item(&self) -> ContentId<OracleItemArtifact> {
        self.item
    }

    #[must_use]
    pub const fn method(&self) -> OracleCheckMethodV1 {
        self.method
    }

    /// Returns the machine-evaluable half of this plan's pass condition.
    #[must_use]
    pub const fn assertion(&self) -> OracleCheckAssertionV1 {
        self.assertion
    }

    #[must_use]
    pub fn evidence(&self) -> &[OracleCheckEvidenceV1] {
        &self.evidence
    }

    pub fn identity(&self) -> Result<ContentId<OracleCheckPlanArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        self.assertion.validate()?;
        if self.evidence.is_empty() {
            return Err(OracleFrameworkError::Empty("Oracle check evidence"));
        }
        let encoded = self
            .evidence
            .iter()
            .map(cairn_codec::to_vec)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| OracleFrameworkError::Codec(error.to_string()))?;
        if encoded.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(OracleFrameworkError::NonCanonical("Oracle check evidence"));
        }
        Ok(())
    }
}

impl TryFrom<OracleCheckPlanWire> for OracleCheckPlanV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleCheckPlanWire) -> Result<Self, Self::Error> {
        require_v1(wire.schema_version)?;
        Self::new(
            wire.item,
            wire.method,
            wire.objective,
            wire.setup,
            wire.observation,
            wire.pass_condition,
            wire.assertion,
            wire.evidence,
        )
    }
}

impl<'de> Deserialize<'de> for OracleCheckPlanV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleCheckPlanWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// One immutable generation from an exact item-scoped conversation. Review always binds this
/// exact draft identity; feedback cannot be applied to a sibling item or an older revision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleItemDraftV1 {
    schema_version: u16,
    item: OracleItemV1,
    run: ContentId<OracleStrategyRunArtifact>,
    parent: Option<ContentId<OracleItemDraftArtifact>>,
    revision: OracleItemRevision,
    plans: Vec<OracleCheckPlanV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleItemDraftWire {
    schema_version: u16,
    item: OracleItemV1,
    run: ContentId<OracleStrategyRunArtifact>,
    parent: Option<ContentId<OracleItemDraftArtifact>>,
    revision: OracleItemRevision,
    plans: Vec<OracleCheckPlanV1>,
}

impl OracleItemDraftV1 {
    pub fn initial(
        item: OracleItemV1,
        run: ContentId<OracleStrategyRunArtifact>,
        plans: Vec<OracleCheckPlanV1>,
    ) -> Result<Self, OracleFrameworkError> {
        Self::build(item, run, None, OracleItemRevision::new(1)?, plans)
    }

    pub fn revise(
        previous: &Self,
        run: ContentId<OracleStrategyRunArtifact>,
        plans: Vec<OracleCheckPlanV1>,
    ) -> Result<Self, OracleFrameworkError> {
        Self::build(
            previous.item.clone(),
            run,
            Some(previous.identity()?),
            previous.revision.next()?,
            plans,
        )
    }

    fn build(
        item: OracleItemV1,
        run: ContentId<OracleStrategyRunArtifact>,
        parent: Option<ContentId<OracleItemDraftArtifact>>,
        revision: OracleItemRevision,
        plans: Vec<OracleCheckPlanV1>,
    ) -> Result<Self, OracleFrameworkError> {
        let mut identified = plans
            .into_iter()
            .map(|plan| Ok((plan.identity()?.to_wire(), plan)))
            .collect::<Result<Vec<_>, OracleFrameworkError>>()?;
        identified.sort_by(|left, right| left.0.cmp(&right.0));
        let plans = identified.into_iter().map(|(_, plan)| plan).collect();
        let value = Self {
            schema_version: SCHEMA_V1,
            item,
            run,
            parent,
            revision,
            plans,
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub const fn item(&self) -> &OracleItemV1 {
        &self.item
    }

    #[must_use]
    pub const fn run(&self) -> ContentId<OracleStrategyRunArtifact> {
        self.run
    }

    #[must_use]
    pub const fn parent(&self) -> Option<ContentId<OracleItemDraftArtifact>> {
        self.parent
    }

    #[must_use]
    pub const fn revision(&self) -> OracleItemRevision {
        self.revision
    }

    #[must_use]
    pub fn plans(&self) -> &[OracleCheckPlanV1] {
        &self.plans
    }

    pub fn identity(&self) -> Result<ContentId<OracleItemDraftArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        if (self.revision.get() == 1 && self.parent.is_some())
            || (self.revision.get() > 1 && self.parent.is_none())
        {
            return Err(OracleFrameworkError::RevisionLineageMismatch);
        }
        let item = self.item.identity()?;
        let plan_ids = self
            .plans
            .iter()
            .map(|plan| {
                if plan.item() != item {
                    return Err(OracleFrameworkError::OracleItemBindingMismatch);
                }
                plan.identity()
            })
            .collect::<Result<Vec<_>, _>>()?;
        validate_content_ids(&plan_ids, "Oracle item draft plans")
    }
}

impl TryFrom<OracleItemDraftWire> for OracleItemDraftV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleItemDraftWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            item: wire.item,
            run: wire.run,
            parent: wire.parent,
            revision: wire.revision,
            plans: wire.plans,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleItemDraftV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleItemDraftWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Deterministically expands every claim across every policy concern and required logical role.
pub fn derive_oracle_dimensions(
    claims: &[ContentId<OracleClaimArtifact>],
    policy: &OracleCoveragePolicyV1,
) -> Result<Vec<OracleDimensionV1>, OracleFrameworkError> {
    validate_content_ids(claims, "oracle claims")?;
    let mut items = Vec::new();
    for claim in claims {
        for concern in policy.concerns() {
            items.push(OracleDimensionV1::new(
                *claim,
                *concern,
                OracleStrategyRoleV1::Synthesis,
            ));
            if policy.adversarial() == OracleAdversarialPolicyV1::RequiredForEveryConcern {
                items.push(OracleDimensionV1::new(
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

/// One exact strategy execution authority for one indivisible dimension.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleStrategyRunV1 {
    schema_version: u16,
    workspace: ContentId<OracleWorkspaceArtifact>,
    dimension: ContentId<OracleDimensionArtifact>,
    strategy: OracleStrategyName,
    executor: OracleStrategyExecutorV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleStrategyRunWire {
    schema_version: u16,
    workspace: ContentId<OracleWorkspaceArtifact>,
    dimension: ContentId<OracleDimensionArtifact>,
    strategy: OracleStrategyName,
    executor: OracleStrategyExecutorV1,
}

impl OracleStrategyRunV1 {
    pub fn new(
        workspace: ContentId<OracleWorkspaceArtifact>,
        dimension: &OracleDimensionV1,
        strategy: OracleStrategyName,
        catalog: &OracleStrategyCatalogV1,
    ) -> Result<Self, OracleFrameworkError> {
        let executor = catalog.resolve(dimension, &strategy)?.executor.clone();
        Ok(Self {
            schema_version: SCHEMA_V1,
            workspace,
            dimension: dimension.identity()?,
            strategy,
            executor,
        })
    }

    #[must_use]
    pub const fn workspace(&self) -> ContentId<OracleWorkspaceArtifact> {
        self.workspace
    }
    #[must_use]
    pub const fn dimension(&self) -> ContentId<OracleDimensionArtifact> {
        self.dimension
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
        dimension: &OracleDimensionV1,
        catalog: &OracleStrategyCatalogV1,
    ) -> Result<(), OracleFrameworkError> {
        let expected = Self::new(workspace, dimension, self.strategy.clone(), catalog)?;
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
            dimension: wire.dimension,
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
    dimension: ContentId<OracleDimensionArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
    tools: ContentId<OracleExperimentToolCatalogArtifact>,
    operation: OracleExperimentOperationName,
    arguments: ContentId<OracleExperimentArgumentsArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleExperimentRequestWire {
    schema_version: u16,
    dimension: ContentId<OracleDimensionArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
    tools: ContentId<OracleExperimentToolCatalogArtifact>,
    operation: OracleExperimentOperationName,
    arguments: ContentId<OracleExperimentArgumentsArtifact>,
}

impl OracleExperimentRequestV1 {
    #[must_use]
    pub fn new(
        dimension: ContentId<OracleDimensionArtifact>,
        run: ContentId<OracleStrategyRunArtifact>,
        tools: ContentId<OracleExperimentToolCatalogArtifact>,
        operation: OracleExperimentOperationName,
        arguments: ContentId<OracleExperimentArgumentsArtifact>,
    ) -> Self {
        Self {
            schema_version: SCHEMA_V1,
            dimension,
            run,
            tools,
            operation,
            arguments,
        }
    }

    #[must_use]
    pub const fn dimension(&self) -> ContentId<OracleDimensionArtifact> {
        self.dimension
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
            wire.dimension,
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
    item: ContentId<OracleItemArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
    reason: OracleUnknownReason,
    observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleUnknownEvidenceWire {
    schema_version: u16,
    item: ContentId<OracleItemArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
    reason: OracleUnknownReason,
    observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
}

impl OracleUnknownEvidenceV1 {
    pub fn new(
        item: ContentId<OracleItemArtifact>,
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
    pub const fn item(&self) -> ContentId<OracleItemArtifact> {
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
    dimension: ContentId<OracleDimensionArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
    provenance: OracleObservationProvenanceV1,
    payload: ContentId<OracleObservationPayloadArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleExplorationObservationWire {
    schema_version: u16,
    dimension: ContentId<OracleDimensionArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
    provenance: OracleObservationProvenanceV1,
    payload: ContentId<OracleObservationPayloadArtifact>,
}

impl OracleExplorationObservationV1 {
    pub fn workflow_tool(
        dimension: ContentId<OracleDimensionArtifact>,
        run: ContentId<OracleStrategyRunArtifact>,
        source: ContentId<WorkflowToolControllerObservationArtifact>,
        payload: &OracleObservationPayloadV1,
    ) -> Result<Self, OracleFrameworkError> {
        if payload.source() != source {
            return Err(OracleFrameworkError::ObservationBindingMismatch);
        }
        Ok(Self {
            schema_version: SCHEMA_V1,
            dimension,
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
            dimension: request.dimension(),
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
    pub const fn dimension(&self) -> ContentId<OracleDimensionArtifact> {
        self.dimension
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
        Ok(self.dimension == request.dimension()
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
            dimension: wire.dimension,
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
    CheckPlan(ContentId<OracleCheckPlanArtifact>),
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
    item: ContentId<OracleItemArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
    kind: OraclePortfolioElementKindV1,
    observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OraclePortfolioElementWire {
    schema_version: u16,
    item: ContentId<OracleItemArtifact>,
    run: ContentId<OracleStrategyRunArtifact>,
    kind: OraclePortfolioElementKindV1,
    observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
}

impl OraclePortfolioElementV1 {
    pub fn new(
        item: ContentId<OracleItemArtifact>,
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
    pub const fn item(&self) -> ContentId<OracleItemArtifact> {
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
        items: Vec<OracleItemV1>,
        elements: Vec<OraclePortfolioElementV1>,
    },
    RequestExperiment {
        request: OracleExperimentRequestV1,
    },
    PreserveUnknown {
        items: Vec<OracleItemV1>,
        evidence: Vec<OracleUnknownEvidenceV1>,
    },
}

/// Atomic strategy publication bound to one exact run and dimension.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleStrategySubmissionV1 {
    schema_version: u16,
    run: ContentId<OracleStrategyRunArtifact>,
    dimension: ContentId<OracleDimensionArtifact>,
    result: OracleStrategySubmissionOutcomeV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleStrategySubmissionWire {
    schema_version: u16,
    run: ContentId<OracleStrategyRunArtifact>,
    dimension: ContentId<OracleDimensionArtifact>,
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
            dimension: run.dimension(),
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
    pub const fn dimension(&self) -> ContentId<OracleDimensionArtifact> {
        self.dimension
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
            OracleStrategySubmissionOutcomeV1::Contribute { items, elements } => {
                let item_ids = items
                    .iter()
                    .map(|item| {
                        if item.dimension() != self.dimension {
                            return Err(OracleFrameworkError::OracleItemBindingMismatch);
                        }
                        item.identity()
                    })
                    .collect::<Result<HashSet<_>, _>>()?;
                if items.is_empty() || item_ids.len() != items.len() {
                    return Err(OracleFrameworkError::OracleItemSetInvalid);
                }
                let ids = elements
                    .iter()
                    .map(|element| {
                        element.validate()?;
                        if !item_ids.contains(&element.item) || element.run != self.run {
                            return Err(OracleFrameworkError::PortfolioElementBindingMismatch);
                        }
                        element.identity()
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                validate_content_ids(&ids, "strategy contribution")?;
                if item_ids
                    .iter()
                    .any(|item| !elements.iter().any(|element| element.item == *item))
                {
                    return Err(OracleFrameworkError::OracleItemSetInvalid);
                }
                Ok(())
            }
            OracleStrategySubmissionOutcomeV1::RequestExperiment { request } => {
                if request.dimension() != self.dimension || request.run() != self.run {
                    return Err(OracleFrameworkError::ExperimentBindingMismatch);
                }
                Ok(())
            }
            OracleStrategySubmissionOutcomeV1::PreserveUnknown { items, evidence } => {
                let item_ids = items
                    .iter()
                    .map(|item| {
                        if item.dimension() != self.dimension {
                            return Err(OracleFrameworkError::OracleItemBindingMismatch);
                        }
                        item.identity()
                    })
                    .collect::<Result<HashSet<_>, _>>()?;
                let ids = evidence
                    .iter()
                    .map(|value| {
                        if !item_ids.contains(&value.item()) || value.run() != self.run {
                            return Err(OracleFrameworkError::StrategyRunBindingMismatch);
                        }
                        value.identity()
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                validate_content_ids(&ids, "strategy unknown evidence")?;
                if items.is_empty()
                    || item_ids.len() != items.len()
                    || evidence.len() != items.len()
                {
                    return Err(OracleFrameworkError::OracleItemSetInvalid);
                }
                Ok(())
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
            dimension: wire.dimension,
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
        runs: Vec<ContentId<OracleStrategyRunArtifact>>,
        accepted_items: Vec<ContentId<OracleAcceptedItemArtifact>>,
        items: Vec<OracleItemV1>,
        elements: Vec<ContentId<OraclePortfolioElementArtifact>>,
        observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
    },
    CoverageGap {
        run: ContentId<OracleStrategyRunArtifact>,
        elements: Vec<ContentId<OraclePortfolioElementArtifact>>,
        observations: Vec<ContentId<OracleExplorationObservationArtifact>>,
    },
    Unknown {
        items: Vec<OracleItemV1>,
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
    dimension: OracleDimensionV1,
    resolution: OracleObligationResolutionV1,
}

impl OracleObligationEntryV1 {
    #[must_use]
    pub const fn dimension(&self) -> &OracleDimensionV1 {
        &self.dimension
    }
    #[must_use]
    pub const fn resolution(&self) -> &OracleObligationResolutionV1 {
        &self.resolution
    }
}

/// Immutable-snapshot durable exploration ledger. Every revision retains all dimensions.
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
        dimensions: Vec<OracleDimensionV1>,
        catalog: &OracleStrategyCatalogV1,
    ) -> Result<Self, OracleFrameworkError> {
        validate_strict(&dimensions, "oracle dimensions")?;
        for item in &dimensions {
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
            entries: dimensions
                .into_iter()
                .map(|item| OracleObligationEntryV1 {
                    dimension: item,
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
        let index = self.entry_index(run.dimension())?;
        if !matches!(
            self.entries[index].resolution,
            OracleObligationResolutionV1::Pending
        ) {
            return Err(OracleFrameworkError::InvalidLedgerTransition);
        }
        run.validate_against(self.workspace, &self.entries[index].dimension, catalog)?;
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
        dimension: ContentId<OracleDimensionArtifact>,
        run: ContentId<OracleStrategyRunArtifact>,
        observations: &[OracleExplorationObservationV1],
    ) -> Result<Self, OracleFrameworkError> {
        if observations.is_empty()
            || observations
                .iter()
                .any(|observation| observation.dimension() != dimension || observation.run() != run)
        {
            return Err(OracleFrameworkError::ObservationBindingMismatch);
        }
        let mut new_ids = observations
            .iter()
            .map(OracleExplorationObservationV1::identity)
            .collect::<Result<Vec<_>, _>>()?;
        new_ids.sort_by_key(ContentId::to_wire);
        validate_content_ids(&new_ids, "strategy observations")?;
        let index = self.entry_index(dimension)?;
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
        let index = self.entry_index(request.dimension())?;
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
        let index = self.entry_index(request.dimension())?;
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
        let index = self.entry_index(request.dimension())?;
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
            || observation.dimension != request.dimension()
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
        dimension: ContentId<OracleDimensionArtifact>,
        run: ContentId<OracleStrategyRunArtifact>,
        items: &[OracleItemV1],
        elements: &[OraclePortfolioElementV1],
    ) -> Result<Self, OracleFrameworkError> {
        let item_ids = items
            .iter()
            .map(|item| {
                if item.dimension() != dimension {
                    return Err(OracleFrameworkError::OracleItemBindingMismatch);
                }
                item.identity()
            })
            .collect::<Result<HashSet<_>, _>>()?;
        validate_strict(items, "Oracle items")?;
        if items.is_empty()
            || item_ids.len() != items.len()
            || elements
                .iter()
                .any(|element| !item_ids.contains(&element.item) || element.run != run)
            || item_ids
                .iter()
                .any(|item| !elements.iter().any(|element| element.item == *item))
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
        let index = self.entry_index(dimension)?;
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
                    runs: vec![run],
                    accepted_items: Vec::new(),
                    items: items.to_vec(),
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
        dimension: ContentId<OracleDimensionArtifact>,
        run: ContentId<OracleStrategyRunArtifact>,
        items: &[OracleItemV1],
        evidence: &[OracleUnknownEvidenceV1],
    ) -> Result<Self, OracleFrameworkError> {
        let item_ids = items
            .iter()
            .map(|item| {
                if item.dimension() != dimension {
                    return Err(OracleFrameworkError::OracleItemBindingMismatch);
                }
                item.identity()
            })
            .collect::<Result<HashSet<_>, _>>()?;
        validate_strict(items, "unknown Oracle items")?;
        if evidence
            .iter()
            .any(|value| !item_ids.contains(&value.item()) || value.run() != run)
            || items.is_empty()
            || evidence.len() != items.len()
        {
            return Err(OracleFrameworkError::StrategyRunBindingMismatch);
        }
        let mut evidence_ids = evidence
            .iter()
            .map(OracleUnknownEvidenceV1::identity)
            .collect::<Result<Vec<_>, _>>()?;
        evidence_ids.sort_by_key(ContentId::to_wire);
        validate_content_ids(&evidence_ids, "unknown evidence")?;
        let index = self.entry_index(dimension)?;
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
                items: items.to_vec(),
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
            || run.dimension() != submission.dimension()
            || run.workspace() != self.workspace
            || workspace.identity()? != self.workspace
        {
            return Err(OracleFrameworkError::StrategyRunBindingMismatch);
        }
        match submission.result() {
            OracleStrategySubmissionOutcomeV1::Contribute { items, elements } => {
                self.record_contribution(run.dimension(), submission.run(), items, elements)
            }
            OracleStrategySubmissionOutcomeV1::RequestExperiment { request } => {
                self.request_experiment(request, workspace)
            }
            OracleStrategySubmissionOutcomeV1::PreserveUnknown { items, evidence } => {
                self.record_unknown(run.dimension(), submission.run(), items, evidence)
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
                dimension: entry.dimension.clone(),
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
            let strategies = catalog.eligible(&entry.dimension);
            if strategies.is_empty() {
                return Err(OracleFrameworkError::MissingStrategy {
                    plane: entry.dimension.plane,
                    concern: entry.dimension.concern,
                    role: entry.dimension.role,
                });
            }
            return Ok(OracleExplorationNextActionV1::RunStrategy {
                dimension: entry.dimension.clone(),
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
            .map(|entry| entry.dimension.clone())
            .collect();
        validate_strict(&items, "oracle ledger dimensions")?;
        for entry in &self.entries {
            match &entry.resolution {
                OracleObligationResolutionV1::Contributed { elements, .. }
                | OracleObligationResolutionV1::CoverageGap { elements, .. }
                    if elements.is_empty() =>
                {
                    return Err(OracleFrameworkError::Empty("portfolio contribution"));
                }
                OracleObligationResolutionV1::Contributed {
                    runs,
                    accepted_items,
                    items,
                    elements,
                    observations,
                    ..
                } => {
                    validate_content_ids(runs, "Oracle contribution runs")?;
                    validate_content_id_order(accepted_items, "accepted Oracle item lineage")?;
                    validate_strict(items, "Oracle items")?;
                    let dimension = entry.dimension.identity()?;
                    if items.iter().any(|item| item.dimension() != dimension) {
                        return Err(OracleFrameworkError::OracleItemBindingMismatch);
                    }
                    if accepted_items.len() > items.len() {
                        return Err(OracleFrameworkError::OracleItemSetInvalid);
                    }
                    validate_content_ids(elements, "portfolio elements")?;
                    validate_content_id_order(observations, "exploration observations")?;
                }
                OracleObligationResolutionV1::CoverageGap {
                    elements,
                    observations,
                    ..
                } => {
                    validate_content_ids(elements, "portfolio elements")?;
                    validate_content_id_order(observations, "exploration observations")?;
                }
                OracleObligationResolutionV1::Unknown {
                    items, evidence, ..
                } => {
                    validate_strict(items, "unknown Oracle items")?;
                    let dimension = entry.dimension.identity()?;
                    if items.iter().any(|item| item.dimension() != dimension) {
                        return Err(OracleFrameworkError::OracleItemBindingMismatch);
                    }
                    validate_content_ids(evidence, "unknown evidence")?;
                }
                OracleObligationResolutionV1::Unsupported { evidence } => {
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
        dimension: ContentId<OracleDimensionArtifact>,
    ) -> Result<usize, OracleFrameworkError> {
        self.entries
            .iter()
            .position(|entry| {
                entry
                    .dimension
                    .identity()
                    .is_ok_and(|identity| identity == dimension)
            })
            .ok_or(OracleFrameworkError::UnknownDimension)
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
        dimension: OracleDimensionV1,
        eligible_strategies: Vec<OracleStrategyName>,
    },
    AuthorizeExperiment {
        dimension: OracleDimensionV1,
        request: ContentId<OracleExperimentRequestArtifact>,
    },
    AwaitObservation,
    FreezePortfolio,
    BudgetExhausted,
}
/// Frozen proposal preserves every resolved obligation, including unknowns and policy waivers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OraclePortfolioProposalV1 {
    schema_version: u16,
    workspace: ContentId<OracleWorkspaceArtifact>,
    ledger: ContentId<OracleExplorationLedgerArtifact>,
    entries: Vec<OracleObligationEntryV1>,
    accepted_items: Vec<OracleAcceptedItemV1>,
    elements: Vec<OraclePortfolioElementV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OraclePortfolioProposalWire {
    schema_version: u16,
    workspace: ContentId<OracleWorkspaceArtifact>,
    ledger: ContentId<OracleExplorationLedgerArtifact>,
    entries: Vec<OracleObligationEntryV1>,
    accepted_items: Vec<OracleAcceptedItemV1>,
    elements: Vec<OraclePortfolioElementV1>,
}

impl OraclePortfolioProposalV1 {
    /// Mechanically assembles exact item revisions carrying treatment-appropriate proposal
    /// authority. This does not grant Oracle Admission.
    pub fn assemble(
        workspace: &OracleWorkspaceV1,
        mut dimensions: Vec<OracleDimensionV1>,
        accepted_items: Vec<OracleAcceptedItemV1>,
    ) -> Result<Self, OracleFrameworkError> {
        dimensions.sort();
        validate_strict(&dimensions, "reviewed Oracle dimensions")?;
        let mut identified_items = accepted_items
            .into_iter()
            .map(|accepted| Ok((accepted.identity()?.to_wire(), accepted)))
            .collect::<Result<Vec<_>, OracleFrameworkError>>()?;
        identified_items.sort_by(|left, right| left.0.cmp(&right.0));
        let accepted_items = identified_items
            .into_iter()
            .map(|(_, accepted)| accepted)
            .collect::<Vec<_>>();
        let accepted_item_ids = accepted_items
            .iter()
            .map(OracleAcceptedItemV1::identity)
            .collect::<Result<Vec<_>, _>>()?;
        validate_content_ids(&accepted_item_ids, "reviewed Oracle items")?;

        let mut elements = Vec::new();
        let mut entries = Vec::new();
        for dimension in dimensions {
            let dimension_id = dimension.identity()?;
            let accepted = accepted_items
                .iter()
                .filter(|accepted| accepted.item().dimension() == dimension_id)
                .collect::<Vec<_>>();
            if accepted.is_empty() {
                return Err(OracleFrameworkError::OracleItemSetInvalid);
            }
            let mut items = accepted
                .iter()
                .map(|accepted| accepted.item().clone())
                .collect::<Vec<_>>();
            items.sort();
            let mut runs = accepted
                .iter()
                .map(|accepted| accepted.run())
                .collect::<Vec<_>>();
            runs.sort_by_key(ContentId::to_wire);
            runs.dedup();
            let mut accepted_ids = accepted
                .iter()
                .map(|accepted| accepted.identity())
                .collect::<Result<Vec<_>, _>>()?;
            accepted_ids.sort_by_key(ContentId::to_wire);
            let mut entry_elements = Vec::new();
            for accepted in accepted {
                let item = accepted.item().identity()?;
                for plan in accepted.plans() {
                    let element = OraclePortfolioElementV1::new(
                        item,
                        accepted.run(),
                        OraclePortfolioElementKindV1::CheckPlan(plan.identity()?),
                        Vec::new(),
                    )?;
                    entry_elements.push(element.identity()?);
                    elements.push(element);
                }
            }
            entry_elements.sort_by_key(ContentId::to_wire);
            entries.push(OracleObligationEntryV1 {
                dimension,
                resolution: OracleObligationResolutionV1::Contributed {
                    runs,
                    accepted_items: accepted_ids,
                    items,
                    elements: entry_elements,
                    observations: Vec::new(),
                },
            });
        }
        let mut identified_elements = elements
            .into_iter()
            .map(|element| Ok((element.identity()?.to_wire(), element)))
            .collect::<Result<Vec<_>, OracleFrameworkError>>()?;
        identified_elements.sort_by(|left, right| left.0.cmp(&right.0));
        let elements = identified_elements
            .into_iter()
            .map(|(_, element)| element)
            .collect();
        let run_count = u32::try_from(accepted_items.len())
            .map_err(|error| OracleFrameworkError::Codec(error.to_string()))?;
        let ledger = OracleExplorationLedgerV1 {
            schema_version: SCHEMA_V1,
            workspace: workspace.identity()?,
            parent: None,
            revision: OracleExplorationRevision::new(1)?,
            entries,
            strategy_runs_started: run_count,
            experiments_started: 0,
        };
        let value = Self {
            schema_version: SCHEMA_V1,
            workspace: workspace.identity()?,
            ledger: ledger.identity()?,
            entries: ledger.entries,
            accepted_items,
            elements,
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub const fn workspace(&self) -> ContentId<OracleWorkspaceArtifact> {
        self.workspace
    }

    #[must_use]
    pub fn entries(&self) -> &[OracleObligationEntryV1] {
        &self.entries
    }

    #[must_use]
    pub fn accepted_items(&self) -> &[OracleAcceptedItemV1] {
        &self.accepted_items
    }

    #[must_use]
    pub fn elements(&self) -> &[OraclePortfolioElementV1] {
        &self.elements
    }

    /// Every independently reviewable/admissible item, in canonical dimension/item order.
    pub fn items(&self) -> impl Iterator<Item = &OracleItemV1> {
        self.entries
            .iter()
            .flat_map(|entry| match &entry.resolution {
                OracleObligationResolutionV1::Contributed { items, .. }
                | OracleObligationResolutionV1::Unknown { items, .. } => items.as_slice(),
                _ => &[],
            })
    }
    pub fn identity(
        &self,
    ) -> Result<ContentId<OraclePortfolioProposalArtifact>, OracleFrameworkError> {
        derive_id(self)
    }
    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        if self.entries.iter().any(|entry| {
            !matches!(
                entry.resolution,
                OracleObligationResolutionV1::Contributed { .. }
            )
        }) {
            return Err(OracleFrameworkError::ExplorationIncomplete);
        }
        let dimensions: Vec<_> = self
            .entries
            .iter()
            .map(|entry| entry.dimension.clone())
            .collect();
        validate_strict(&dimensions, "portfolio dimensions")?;
        let mut all_items = HashSet::new();
        let mut ledger_accepted_items = Vec::new();
        let mut ledger_elements = Vec::new();
        for entry in &self.entries {
            if let OracleObligationResolutionV1::Contributed {
                accepted_items,
                items,
                elements,
                ..
            } = &entry.resolution
            {
                validate_strict(items, "portfolio Oracle items")?;
                if accepted_items.len() != items.len() {
                    return Err(OracleFrameworkError::OracleItemSetInvalid);
                }
                ledger_accepted_items.extend(accepted_items.iter().copied());
                ledger_elements.extend(elements.iter().copied());
                for item in items {
                    if item.dimension() != entry.dimension.identity()?
                        || !all_items.insert(item.identity()?)
                    {
                        return Err(OracleFrameworkError::OracleItemBindingMismatch);
                    }
                }
            }
        }
        ledger_accepted_items.sort_by_key(ContentId::to_wire);
        ledger_elements.sort_by_key(ContentId::to_wire);
        let mut accepted_ids = self
            .accepted_items
            .iter()
            .map(OracleAcceptedItemV1::identity)
            .collect::<Result<Vec<_>, _>>()?;
        accepted_ids.sort_by_key(ContentId::to_wire);
        let mut element_ids = self
            .elements
            .iter()
            .map(OraclePortfolioElementV1::identity)
            .collect::<Result<Vec<_>, _>>()?;
        element_ids.sort_by_key(ContentId::to_wire);
        if accepted_ids != ledger_accepted_items || element_ids != ledger_elements {
            return Err(OracleFrameworkError::PortfolioElementBindingMismatch);
        }
        let accepted_item_ids = self
            .accepted_items
            .iter()
            .map(|accepted| accepted.item().identity())
            .collect::<Result<HashSet<_>, _>>()?;
        if accepted_item_ids != all_items {
            return Err(OracleFrameworkError::OracleItemBindingMismatch);
        }
        Ok(())
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
            accepted_items: wire.accepted_items,
            elements: wire.elements,
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

/// Cross-item issue class considered only after every exact item draft passed its own Review.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OraclePortfolioCoherenceIssueClassV1 {
    ContradictoryItems,
    DuplicateCoverage,
    ConflictingPassConditions,
    CrossPlaneGap,
    JointCoverageGap,
}

/// Non-empty canonical set of existing Oracle items affected by one cross-item finding.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OracleAffectedItemSetV1(Vec<ContentId<OracleItemArtifact>>);

impl OracleAffectedItemSetV1 {
    pub fn new(
        mut items: Vec<ContentId<OracleItemArtifact>>,
    ) -> Result<Self, OracleFrameworkError> {
        items.sort_by_key(ContentId::to_wire);
        validate_content_ids(&items, "affected Oracle item set")?;
        Ok(Self(items))
    }

    #[must_use]
    pub fn items(&self) -> &[ContentId<OracleItemArtifact>] {
        &self.0
    }
}

impl<'de> Deserialize<'de> for OracleAffectedItemSetV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Vec::<ContentId<OracleItemArtifact>>::deserialize(
            deserializer,
        )?)
        .map_err(de::Error::custom)
    }
}

/// One actionable semantic relationship failure over a non-empty exact item set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePortfolioCoherenceFindingV1 {
    affected_items: OracleAffectedItemSetV1,
    issue: OraclePortfolioCoherenceIssueClassV1,
    explanation: OracleReviewExplanation,
    required_change: OracleReviewRequiredChange,
}

impl OraclePortfolioCoherenceFindingV1 {
    #[must_use]
    pub const fn new(
        affected_items: OracleAffectedItemSetV1,
        issue: OraclePortfolioCoherenceIssueClassV1,
        explanation: OracleReviewExplanation,
        required_change: OracleReviewRequiredChange,
    ) -> Self {
        Self {
            affected_items,
            issue,
            explanation,
            required_change,
        }
    }

    #[must_use]
    pub const fn affected_items(&self) -> &OracleAffectedItemSetV1 {
        &self.affected_items
    }

    #[must_use]
    pub const fn issue(&self) -> OraclePortfolioCoherenceIssueClassV1 {
        self.issue
    }

    #[must_use]
    pub const fn explanation(&self) -> &OracleReviewExplanation {
        &self.explanation
    }

    #[must_use]
    pub const fn required_change(&self) -> &OracleReviewRequiredChange {
        &self.required_change
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "decision", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OraclePortfolioCoherenceDecisionV1 {
    Approved,
    NeedsRevision {
        findings: Vec<OraclePortfolioCoherenceFindingV1>,
    },
}

/// Narrow semantic Review of cross-item relationships in one exact mechanically assembled
/// portfolio. It grants neither control receipt nor Admission authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OraclePortfolioCoherenceReviewV1 {
    schema_version: u16,
    portfolio: ContentId<OraclePortfolioProposalArtifact>,
    decision: OraclePortfolioCoherenceDecisionV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OraclePortfolioCoherenceReviewWire {
    schema_version: u16,
    portfolio: ContentId<OraclePortfolioProposalArtifact>,
    decision: OraclePortfolioCoherenceDecisionV1,
}

impl OraclePortfolioCoherenceReviewV1 {
    pub fn approved(portfolio: &OraclePortfolioProposalV1) -> Result<Self, OracleFrameworkError> {
        Ok(Self {
            schema_version: SCHEMA_V1,
            portfolio: portfolio.identity()?,
            decision: OraclePortfolioCoherenceDecisionV1::Approved,
        })
    }

    pub fn needs_revision(
        portfolio: &OraclePortfolioProposalV1,
        findings: Vec<OraclePortfolioCoherenceFindingV1>,
    ) -> Result<Self, OracleFrameworkError> {
        let mut encoded = findings
            .into_iter()
            .map(|finding| {
                cairn_codec::to_vec(&finding)
                    .map(|bytes| (bytes, finding))
                    .map_err(|error| OracleFrameworkError::Codec(error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        encoded.sort_by(|left, right| left.0.cmp(&right.0));
        if encoded.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(OracleFrameworkError::ReviewFindingsInvalid);
        }
        let findings = encoded.into_iter().map(|(_, finding)| finding).collect();
        let value = Self {
            schema_version: SCHEMA_V1,
            portfolio: portfolio.identity()?,
            decision: OraclePortfolioCoherenceDecisionV1::NeedsRevision { findings },
        };
        value.validate_against(portfolio)?;
        Ok(value)
    }

    #[must_use]
    pub const fn portfolio(&self) -> ContentId<OraclePortfolioProposalArtifact> {
        self.portfolio
    }

    #[must_use]
    pub const fn decision(&self) -> &OraclePortfolioCoherenceDecisionV1 {
        &self.decision
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OraclePortfolioCoherenceReviewArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    pub fn validate_against(
        &self,
        portfolio: &OraclePortfolioProposalV1,
    ) -> Result<(), OracleFrameworkError> {
        self.validate()?;
        if self.portfolio != portfolio.identity()? {
            return Err(OracleFrameworkError::ReviewProposalMismatch);
        }
        let item_ids = portfolio
            .accepted_items()
            .iter()
            .map(|accepted| accepted.item().identity())
            .collect::<Result<HashSet<_>, _>>()?;
        if let OraclePortfolioCoherenceDecisionV1::NeedsRevision { findings } = &self.decision {
            if findings.iter().any(|finding| {
                finding
                    .affected_items()
                    .items()
                    .iter()
                    .any(|item| !item_ids.contains(item))
            }) {
                return Err(OracleFrameworkError::ReviewFindingsInvalid);
            }
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        if let OraclePortfolioCoherenceDecisionV1::NeedsRevision { findings } = &self.decision {
            if findings.is_empty() {
                return Err(OracleFrameworkError::ReviewFindingsInvalid);
            }
            let encoded = findings
                .iter()
                .map(cairn_codec::to_vec)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| OracleFrameworkError::Codec(error.to_string()))?;
            if encoded.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(OracleFrameworkError::ReviewFindingsInvalid);
            }
        }
        Ok(())
    }
}

impl TryFrom<OraclePortfolioCoherenceReviewWire> for OraclePortfolioCoherenceReviewV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OraclePortfolioCoherenceReviewWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            portfolio: wire.portfolio,
            decision: wire.decision,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OraclePortfolioCoherenceReviewV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OraclePortfolioCoherenceReviewWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Exact portfolio state allowed to proceed to qualified controls only after the narrow
/// cross-item Review approved that same immutable proposal.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleCoherentPortfolioV1 {
    schema_version: u16,
    proposal: OraclePortfolioProposalV1,
    review: OraclePortfolioCoherenceReviewV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleCoherentPortfolioWire {
    schema_version: u16,
    proposal: OraclePortfolioProposalV1,
    review: OraclePortfolioCoherenceReviewV1,
}

impl OracleCoherentPortfolioV1 {
    pub fn new(
        proposal: &OraclePortfolioProposalV1,
        review: &OraclePortfolioCoherenceReviewV1,
    ) -> Result<Self, OracleFrameworkError> {
        review.validate_against(proposal)?;
        if !matches!(
            review.decision(),
            OraclePortfolioCoherenceDecisionV1::Approved
        ) {
            return Err(OracleFrameworkError::ReviewCannotApproveUnresolved);
        }
        Ok(Self {
            schema_version: SCHEMA_V1,
            proposal: proposal.clone(),
            review: review.clone(),
        })
    }

    #[must_use]
    pub const fn proposal(&self) -> &OraclePortfolioProposalV1 {
        &self.proposal
    }

    #[must_use]
    pub const fn review(&self) -> &OraclePortfolioCoherenceReviewV1 {
        &self.review
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleCoherentPortfolioArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        self.review.validate_against(&self.proposal)?;
        if !matches!(
            self.review.decision(),
            OraclePortfolioCoherenceDecisionV1::Approved
        ) {
            return Err(OracleFrameworkError::ReviewCannotApproveUnresolved);
        }
        Ok(())
    }
}

impl TryFrom<OracleCoherentPortfolioWire> for OracleCoherentPortfolioV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleCoherentPortfolioWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            proposal: wire.proposal,
            review: wire.review,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleCoherentPortfolioV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleCoherentPortfolioWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Review issue class for one exact Oracle item.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleReviewIssueClassV1 {
    UnresolvedUnknown,
    ConcernMismatch,
    UnsupportedEvidence,
    ObjectiveIncomplete,
    SetupIncomplete,
    ObservationUnexecutable,
    PassConditionAmbiguous,
}

/// Actionable reviewer feedback bound to one exact item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleReviewFindingV1 {
    #[serde(rename = "item_id")]
    item: ContentId<OracleItemArtifact>,
    issue: OracleReviewIssueClassV1,
    explanation: OracleReviewExplanation,
    required_change: OracleReviewRequiredChange,
}

impl OracleReviewFindingV1 {
    #[must_use]
    pub const fn new(
        item: ContentId<OracleItemArtifact>,
        issue: OracleReviewIssueClassV1,
        explanation: OracleReviewExplanation,
        required_change: OracleReviewRequiredChange,
    ) -> Self {
        Self {
            item,
            issue,
            explanation,
            required_change,
        }
    }

    #[must_use]
    pub const fn item(&self) -> ContentId<OracleItemArtifact> {
        self.item
    }

    #[must_use]
    pub const fn issue(&self) -> OracleReviewIssueClassV1 {
        self.issue
    }

    #[must_use]
    pub fn explanation(&self) -> &OracleReviewExplanation {
        &self.explanation
    }

    #[must_use]
    pub fn required_change(&self) -> &OracleReviewRequiredChange {
        &self.required_change
    }
}

/// Independent Review of one exact Oracle item draft revision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleItemReviewV1 {
    schema_version: u16,
    item: ContentId<OracleItemArtifact>,
    draft: ContentId<OracleItemDraftArtifact>,
    decision: OracleItemReviewDecisionV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "decision", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OracleItemReviewDecisionV1 {
    Approved,
    NeedsRevision {
        findings: Vec<OracleReviewFindingV1>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleItemReviewWire {
    schema_version: u16,
    item: ContentId<OracleItemArtifact>,
    draft: ContentId<OracleItemDraftArtifact>,
    decision: OracleItemReviewDecisionV1,
}

impl OracleItemReviewV1 {
    pub fn approved(draft: &OracleItemDraftV1) -> Result<Self, OracleFrameworkError> {
        Ok(Self {
            schema_version: SCHEMA_V1,
            item: draft.item().identity()?,
            draft: draft.identity()?,
            decision: OracleItemReviewDecisionV1::Approved,
        })
    }

    pub fn needs_revision(
        draft: &OracleItemDraftV1,
        findings: Vec<OracleReviewFindingV1>,
    ) -> Result<Self, OracleFrameworkError> {
        let item = draft.item().identity()?;
        if findings.is_empty() || findings.iter().any(|finding| finding.item() != item) {
            return Err(OracleFrameworkError::ReviewFindingsInvalid);
        }
        let mut encoded = findings
            .into_iter()
            .map(|finding| {
                cairn_codec::to_vec(&finding)
                    .map(|encoded| (encoded, finding))
                    .map_err(|error| OracleFrameworkError::Codec(error.to_string()))
            })
            .collect::<Result<Vec<_>, OracleFrameworkError>>()?;
        encoded.sort_by(|left, right| left.0.cmp(&right.0));
        if encoded.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(OracleFrameworkError::ReviewFindingsInvalid);
        }
        let findings = encoded.into_iter().map(|(_, finding)| finding).collect();
        Ok(Self {
            schema_version: SCHEMA_V1,
            item,
            draft: draft.identity()?,
            decision: OracleItemReviewDecisionV1::NeedsRevision { findings },
        })
    }

    #[must_use]
    pub const fn item(&self) -> ContentId<OracleItemArtifact> {
        self.item
    }

    #[must_use]
    pub const fn draft(&self) -> ContentId<OracleItemDraftArtifact> {
        self.draft
    }

    #[must_use]
    pub const fn decision(&self) -> &OracleItemReviewDecisionV1 {
        &self.decision
    }

    pub fn identity(&self) -> Result<ContentId<OracleItemReviewArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    pub fn validate_against(&self, draft: &OracleItemDraftV1) -> Result<(), OracleFrameworkError> {
        self.validate()?;
        if self.item != draft.item().identity()? || self.draft != draft.identity()? {
            return Err(OracleFrameworkError::ReviewProposalMismatch);
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        match &self.decision {
            OracleItemReviewDecisionV1::Approved => Ok(()),
            OracleItemReviewDecisionV1::NeedsRevision { findings } => {
                if findings.is_empty() || findings.iter().any(|finding| finding.item() != self.item)
                {
                    return Err(OracleFrameworkError::ReviewFindingsInvalid);
                }
                let encoded = findings
                    .iter()
                    .map(cairn_codec::to_vec)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| OracleFrameworkError::Codec(error.to_string()))?;
                if encoded.windows(2).any(|pair| pair[0] >= pair[1]) {
                    return Err(OracleFrameworkError::ReviewFindingsInvalid);
                }
                Ok(())
            }
        }
    }
}

impl TryFrom<OracleItemReviewWire> for OracleItemReviewV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleItemReviewWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            item: wire.item,
            draft: wire.draft,
            decision: wire.decision,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleItemReviewV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleItemReviewWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Controller authority for one whole-portfolio proposal episode in the minimal-decomposition arm.
///
/// This is proposal authority, not independent Review or Oracle Admission. Its distinct identity
/// prevents an ablation arm from fabricating an `Approved` review that never occurred.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleWholePortfolioProposalAuthorityV1 {
    schema_version: u16,
    workspace: ContentId<OracleWorkspaceArtifact>,
    dimensions: Vec<ContentId<OracleDimensionArtifact>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleWholePortfolioProposalAuthorityWire {
    schema_version: u16,
    workspace: ContentId<OracleWorkspaceArtifact>,
    dimensions: Vec<ContentId<OracleDimensionArtifact>>,
}

impl OracleWholePortfolioProposalAuthorityV1 {
    pub fn new(
        workspace: &OracleWorkspaceV1,
        mut dimensions: Vec<ContentId<OracleDimensionArtifact>>,
    ) -> Result<Self, OracleFrameworkError> {
        dimensions.sort_by_key(ContentId::to_wire);
        validate_content_ids(&dimensions, "whole-portfolio proposal dimensions")?;
        let value = Self {
            schema_version: SCHEMA_V1,
            workspace: workspace.identity()?,
            dimensions,
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub const fn workspace(&self) -> ContentId<OracleWorkspaceArtifact> {
        self.workspace
    }

    #[must_use]
    pub fn dimensions(&self) -> &[ContentId<OracleDimensionArtifact>] {
        &self.dimensions
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleWholePortfolioProposalAuthorityArtifact>, OracleFrameworkError>
    {
        derive_id(self)
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        validate_content_ids(&self.dimensions, "whole-portfolio proposal dimensions")
    }
}

impl TryFrom<OracleWholePortfolioProposalAuthorityWire>
    for OracleWholePortfolioProposalAuthorityV1
{
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleWholePortfolioProposalAuthorityWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            workspace: wire.workspace,
            dimensions: wire.dimensions,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleWholePortfolioProposalAuthorityV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleWholePortfolioProposalAuthorityWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Exact authority by which one item entered a proposal portfolio.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum OracleItemProposalAuthorityV1 {
    IndependentReview {
        review: OracleItemReviewV1,
    },
    WholePortfolioEpisode {
        authority: OracleWholePortfolioProposalAuthorityV1,
    },
}

/// Item admitted into a proposal portfolio through the treatment-appropriate proposal authority.
/// Neither authority variant grants Oracle Admission.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleAcceptedItemV1 {
    schema_version: u16,
    draft: OracleItemDraftV1,
    authority: OracleItemProposalAuthorityV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleAcceptedItemWire {
    schema_version: u16,
    draft: OracleItemDraftV1,
    authority: OracleItemProposalAuthorityV1,
}

impl OracleAcceptedItemV1 {
    pub fn new(
        draft: &OracleItemDraftV1,
        review: &OracleItemReviewV1,
    ) -> Result<Self, OracleFrameworkError> {
        review.validate_against(draft)?;
        if !matches!(review.decision(), OracleItemReviewDecisionV1::Approved) {
            return Err(OracleFrameworkError::ReviewCannotApproveUnresolved);
        }
        Ok(Self {
            schema_version: SCHEMA_V1,
            draft: draft.clone(),
            authority: OracleItemProposalAuthorityV1::IndependentReview {
                review: review.clone(),
            },
        })
    }

    pub fn from_whole_portfolio_episode(
        draft: &OracleItemDraftV1,
        authority: &OracleWholePortfolioProposalAuthorityV1,
    ) -> Result<Self, OracleFrameworkError> {
        let value = Self {
            schema_version: SCHEMA_V1,
            draft: draft.clone(),
            authority: OracleItemProposalAuthorityV1::WholePortfolioEpisode {
                authority: authority.clone(),
            },
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub const fn item(&self) -> &OracleItemV1 {
        self.draft.item()
    }

    #[must_use]
    pub const fn draft(&self) -> &OracleItemDraftV1 {
        &self.draft
    }

    #[must_use]
    pub const fn authority(&self) -> &OracleItemProposalAuthorityV1 {
        &self.authority
    }

    #[must_use]
    pub const fn run(&self) -> ContentId<OracleStrategyRunArtifact> {
        self.draft.run()
    }

    #[must_use]
    pub fn plans(&self) -> &[OracleCheckPlanV1] {
        self.draft.plans()
    }

    pub fn identity(&self) -> Result<ContentId<OracleAcceptedItemArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        self.draft.validate()?;
        match &self.authority {
            OracleItemProposalAuthorityV1::IndependentReview { review } => {
                review.validate_against(&self.draft)?;
                if !matches!(review.decision(), OracleItemReviewDecisionV1::Approved) {
                    return Err(OracleFrameworkError::ReviewCannotApproveUnresolved);
                }
            }
            OracleItemProposalAuthorityV1::WholePortfolioEpisode { authority } => {
                authority.validate()?;
                if !authority
                    .dimensions()
                    .contains(&self.draft.item().dimension())
                {
                    return Err(OracleFrameworkError::OracleItemBindingMismatch);
                }
            }
        }
        Ok(())
    }
}

impl TryFrom<OracleAcceptedItemWire> for OracleAcceptedItemV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleAcceptedItemWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            draft: wire.draft,
            authority: wire.authority,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleAcceptedItemV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleAcceptedItemWire::deserialize(deserializer)?
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
    item: ContentId<OracleItemArtifact>,
    control: OracleControlFamilyV1,
    mechanism: ContentId<OracleQualifiedMechanismArtifact>,
}

impl OracleControlObligationV1 {
    #[must_use]
    pub const fn item(&self) -> ContentId<OracleItemArtifact> {
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
        for item in proposal.items() {
            for control in policy.required_controls() {
                required_controls.push(OracleControlObligationV1 {
                    item: item.identity()?,
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

/// Bounded, non-authoritative diagnostic projected from one exact trusted execution receipt.
/// Artifact identities preserve access to the original untrusted output without logging it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleControlFailureClassV1 {
    /// The honest control rejected the submitted Oracle artifact itself.
    OracleArtifactRejected,
    /// A negative control accepted the deliberately invalid challenge it was required to reject.
    NegativeChallengeAccepted,
    /// The qualified mechanism violated its own control protocol.
    MechanismProtocolViolation,
    /// Worker execution failed before a trustworthy control decision was available.
    ExecutionFailure,
}

impl OracleControlFailureClassV1 {
    #[must_use]
    pub const fn requires_oracle_revision(self) -> bool {
        matches!(self, Self::OracleArtifactRejected)
    }

    #[must_use]
    pub const fn requires_control_reconciliation(self) -> bool {
        !self.requires_oracle_revision()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleControlDiagnosticV1 {
    failure_class: OracleControlFailureClassV1,
    summary: OracleControlDiagnosticSummary,
    stdout: ContentId<ExecutionStdoutArtifact>,
    stderr: ContentId<ExecutionStderrArtifact>,
}

impl OracleControlDiagnosticV1 {
    #[must_use]
    pub const fn new(
        failure_class: OracleControlFailureClassV1,
        summary: OracleControlDiagnosticSummary,
        stdout: ContentId<ExecutionStdoutArtifact>,
        stderr: ContentId<ExecutionStderrArtifact>,
    ) -> Self {
        Self {
            failure_class,
            summary,
            stdout,
            stderr,
        }
    }

    #[must_use]
    pub const fn failure_class(&self) -> OracleControlFailureClassV1 {
        self.failure_class
    }

    #[must_use]
    pub const fn summary(&self) -> &OracleControlDiagnosticSummary {
        &self.summary
    }

    #[must_use]
    pub const fn stdout(&self) -> ContentId<ExecutionStdoutArtifact> {
        self.stdout
    }

    #[must_use]
    pub const fn stderr(&self) -> ContentId<ExecutionStderrArtifact> {
        self.stderr
    }
}

/// Controller-validated receipt for one exact portfolio item and control family.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OracleControlReceiptV1 {
    proposal: ContentId<OraclePortfolioProposalArtifact>,
    item: ContentId<OracleItemArtifact>,
    control: OracleControlFamilyV1,
    mechanism: ContentId<OracleQualifiedMechanismArtifact>,
    receipt: ContentId<TrustedOracleControlReceiptArtifact>,
    result: OracleControlResultV1,
    diagnostic: Option<OracleControlDiagnosticV1>,
}

impl OracleControlReceiptV1 {
    pub fn new(
        proposal: ContentId<OraclePortfolioProposalArtifact>,
        item: ContentId<OracleItemArtifact>,
        control: OracleControlFamilyV1,
        mechanism: ContentId<OracleQualifiedMechanismArtifact>,
        receipt: ContentId<TrustedOracleControlReceiptArtifact>,
        result: OracleControlResultV1,
        diagnostic: Option<OracleControlDiagnosticV1>,
    ) -> Result<Self, OracleFrameworkError> {
        if (result == OracleControlResultV1::Passed) != diagnostic.is_none()
            || diagnostic
                .as_ref()
                .is_some_and(|diagnostic| !failure_class_matches_control(control, diagnostic))
        {
            return Err(OracleFrameworkError::AdmissionEvidenceBindingMismatch);
        }
        Ok(Self {
            proposal,
            item,
            control,
            mechanism,
            receipt,
            result,
            diagnostic,
        })
    }

    pub fn from_trusted_observation(
        proposal: ContentId<OraclePortfolioProposalArtifact>,
        run: &OracleControlRunV1,
        observation: &TrustedOracleControlObservationV1,
        failure_class: Option<OracleControlFailureClassV1>,
    ) -> Result<Self, OracleFrameworkError> {
        if observation.run()
            != run
                .identity()
                .map_err(|error| OracleFrameworkError::Codec(error.to_string()))?
        {
            return Err(OracleFrameworkError::ReceiptBindingMismatch);
        }
        if (observation.result() == OracleControlResultV1::Passed) != failure_class.is_none() {
            return Err(OracleFrameworkError::AdmissionEvidenceBindingMismatch);
        }
        let diagnostic = failure_class
            .map(|failure_class| {
                let receipt = observation.receipt();
                let explanation = match failure_class {
                    OracleControlFailureClassV1::OracleArtifactRejected => {
                        "the honest control rejected the submitted Oracle artifact"
                    }
                    OracleControlFailureClassV1::NegativeChallengeAccepted => {
                        "the negative control accepted its deliberately invalid challenge"
                    }
                    OracleControlFailureClassV1::MechanismProtocolViolation => {
                        "the qualified mechanism violated its control protocol"
                    }
                    OracleControlFailureClassV1::ExecutionFailure => {
                        "Worker execution failed before a trustworthy control decision"
                    }
                };
                Ok::<_, OracleFrameworkError>(OracleControlDiagnosticV1::new(
                    failure_class,
                    OracleControlDiagnosticSummary::new(format!(
                        "{explanation}; control {:?}, outcome {:?}, exit code {:?}; inspect the exact stdout/stderr artifacts",
                        run.obligation().control(),
                        receipt.outcome(),
                        receipt.exit_code(),
                    ))?,
                    receipt.stdout_id(),
                    receipt.stderr_id(),
                ))
            })
            .transpose()?;
        Self::new(
            proposal,
            run.obligation().item(),
            run.obligation().control(),
            run.obligation().mechanism(),
            observation
                .identity()
                .map_err(|error| OracleFrameworkError::Codec(error.to_string()))?,
            observation.result(),
            diagnostic,
        )
    }

    #[must_use]
    pub const fn proposal(&self) -> ContentId<OraclePortfolioProposalArtifact> {
        self.proposal
    }

    #[must_use]
    pub const fn item(&self) -> ContentId<OracleItemArtifact> {
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

    #[must_use]
    pub const fn diagnostic(&self) -> Option<&OracleControlDiagnosticV1> {
        self.diagnostic.as_ref()
    }

    #[must_use]
    pub fn failure_class(&self) -> Option<OracleControlFailureClassV1> {
        self.diagnostic
            .as_ref()
            .map(OracleControlDiagnosticV1::failure_class)
    }
}

fn control_requires_revision(receipt: &OracleControlReceiptV1) -> bool {
    receipt.result() == OracleControlResultV1::Failed
        && receipt
            .failure_class()
            .is_some_and(OracleControlFailureClassV1::requires_oracle_revision)
}

fn control_requires_reconciliation(receipt: &OracleControlReceiptV1) -> bool {
    receipt.result() == OracleControlResultV1::Failed
        && receipt
            .failure_class()
            .is_some_and(OracleControlFailureClassV1::requires_control_reconciliation)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleControlReceiptWire {
    proposal: ContentId<OraclePortfolioProposalArtifact>,
    item: ContentId<OracleItemArtifact>,
    control: OracleControlFamilyV1,
    mechanism: ContentId<OracleQualifiedMechanismArtifact>,
    receipt: ContentId<TrustedOracleControlReceiptArtifact>,
    result: OracleControlResultV1,
    diagnostic: Option<OracleControlDiagnosticV1>,
}

impl TryFrom<OracleControlReceiptWire> for OracleControlReceiptV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleControlReceiptWire) -> Result<Self, Self::Error> {
        Self::new(
            wire.proposal,
            wire.item,
            wire.control,
            wire.mechanism,
            wire.receipt,
            wire.result,
            wire.diagnostic,
        )
    }
}

impl<'de> Deserialize<'de> for OracleControlReceiptV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleControlReceiptWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
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
            || self.receipts.iter().any(|receipt| {
                (receipt.result == OracleControlResultV1::Passed) != receipt.diagnostic.is_none()
                    || receipt.diagnostic.as_ref().is_some_and(|diagnostic| {
                        !failure_class_matches_control(receipt.control, diagnostic)
                    })
            })
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

fn failure_class_matches_control(
    control: OracleControlFamilyV1,
    diagnostic: &OracleControlDiagnosticV1,
) -> bool {
    match diagnostic.failure_class() {
        OracleControlFailureClassV1::OracleArtifactRejected => {
            control == OracleControlFamilyV1::Honest
        }
        OracleControlFailureClassV1::NegativeChallengeAccepted => matches!(
            control,
            OracleControlFamilyV1::Mutant
                | OracleControlFamilyV1::Hidden
                | OracleControlFamilyV1::Bypass
        ),
        OracleControlFailureClassV1::MechanismProtocolViolation
        | OracleControlFailureClassV1::ExecutionFailure => true,
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
    admitted_items: Vec<ContentId<OracleItemArtifact>>,
    unresolved_items: Vec<ContentId<OracleItemArtifact>>,
    rejected_items: Vec<ContentId<OracleItemArtifact>>,
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
    pub fn admitted_items(&self) -> &[ContentId<OracleItemArtifact>] {
        &self.admitted_items
    }

    #[must_use]
    pub fn unresolved_items(&self) -> &[ContentId<OracleItemArtifact>] {
        &self.unresolved_items
    }

    #[must_use]
    pub fn rejected_items(&self) -> &[ContentId<OracleItemArtifact>] {
        &self.rejected_items
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        validate_content_id_order(&self.admitted_items, "admitted Oracle dimensions")?;
        validate_content_id_order(&self.unresolved_items, "unresolved Oracle dimensions")?;
        validate_content_id_order(&self.rejected_items, "rejected Oracle dimensions")?;
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

/// Exact Admission feedback consumed by the affected Oracle item developer loops.
///
/// The complete attempt, outcome, and receipt lineage is carried in the artifact, so a failed
/// qualified control cannot degrade into a bare status or be applied to a sibling item.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleRevisionRequestV1 {
    schema_version: u16,
    attempt: Box<OracleAdmissionAttemptV1>,
    outcome: Box<OracleAdmissionOutcomeV1>,
    evidence: Box<OracleAdmissionEvidenceV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleRevisionRequestWire {
    schema_version: u16,
    attempt: Box<OracleAdmissionAttemptV1>,
    outcome: Box<OracleAdmissionOutcomeV1>,
    evidence: Box<OracleAdmissionEvidenceV1>,
}

impl OracleRevisionRequestV1 {
    pub fn from_admission(
        attempt: OracleAdmissionAttemptV1,
        outcome: OracleAdmissionOutcomeV1,
        evidence: OracleAdmissionEvidenceV1,
    ) -> Result<Self, OracleFrameworkError> {
        if outcome.attempt() != attempt.identity()?
            || evidence.attempt() != attempt.identity()?
            || outcome.evidence() != evidence.identity()?
            || outcome
                .claims()
                .iter()
                .all(|claim| claim.status() == OracleClaimAdmissionStatusV1::Admitted)
            || !evidence.receipts().iter().any(control_requires_revision)
            || evidence
                .receipts()
                .iter()
                .any(control_requires_reconciliation)
            || has_unavailable_or_missing(&attempt, &evidence)
        {
            return Err(OracleFrameworkError::RevisionRequestInvalid);
        }
        Ok(Self {
            schema_version: SCHEMA_V1,
            attempt: Box::new(attempt),
            outcome: Box::new(outcome),
            evidence: Box::new(evidence),
        })
    }

    #[must_use]
    pub const fn attempt(&self) -> &OracleAdmissionAttemptV1 {
        &self.attempt
    }

    #[must_use]
    pub const fn outcome(&self) -> &OracleAdmissionOutcomeV1 {
        &self.outcome
    }

    #[must_use]
    pub const fn evidence(&self) -> &OracleAdmissionEvidenceV1 {
        &self.evidence
    }

    #[must_use]
    pub const fn proposal(&self) -> ContentId<OraclePortfolioProposalArtifact> {
        self.outcome.proposal()
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleRevisionRequestArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        if self.outcome.attempt() != self.attempt.identity()?
            || self.evidence.attempt() != self.attempt.identity()?
            || self.outcome.evidence() != self.evidence.identity()?
            || self
                .outcome
                .claims()
                .iter()
                .all(|claim| claim.status() == OracleClaimAdmissionStatusV1::Admitted)
            || !self
                .evidence
                .receipts()
                .iter()
                .any(control_requires_revision)
            || self
                .evidence
                .receipts()
                .iter()
                .any(control_requires_reconciliation)
            || has_unavailable_or_missing(&self.attempt, &self.evidence)
        {
            return Err(OracleFrameworkError::RevisionRequestInvalid);
        }
        Ok(())
    }
}

impl TryFrom<OracleRevisionRequestWire> for OracleRevisionRequestV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleRevisionRequestWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            attempt: wire.attempt,
            outcome: wire.outcome,
            evidence: wire.evidence,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleRevisionRequestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleRevisionRequestWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Exact Controller reconciliation request for mechanism-owned failures and missing or unavailable
/// control observations.
/// It is intentionally not consumable by an Oracle item developer Agent Loop.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OracleControlReconciliationRequestV1 {
    schema_version: u16,
    attempt: Box<OracleAdmissionAttemptV1>,
    outcome: Box<OracleAdmissionOutcomeV1>,
    evidence: Box<OracleAdmissionEvidenceV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleControlReconciliationRequestWire {
    schema_version: u16,
    attempt: Box<OracleAdmissionAttemptV1>,
    outcome: Box<OracleAdmissionOutcomeV1>,
    evidence: Box<OracleAdmissionEvidenceV1>,
}

impl OracleControlReconciliationRequestV1 {
    pub fn from_admission(
        attempt: OracleAdmissionAttemptV1,
        outcome: OracleAdmissionOutcomeV1,
        evidence: OracleAdmissionEvidenceV1,
    ) -> Result<Self, OracleFrameworkError> {
        let value = Self {
            schema_version: SCHEMA_V1,
            attempt: Box::new(attempt),
            outcome: Box::new(outcome),
            evidence: Box::new(evidence),
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub const fn attempt(&self) -> &OracleAdmissionAttemptV1 {
        &self.attempt
    }

    #[must_use]
    pub const fn outcome(&self) -> &OracleAdmissionOutcomeV1 {
        &self.outcome
    }

    #[must_use]
    pub const fn evidence(&self) -> &OracleAdmissionEvidenceV1 {
        &self.evidence
    }

    #[must_use]
    pub const fn proposal(&self) -> ContentId<OraclePortfolioProposalArtifact> {
        self.outcome.proposal()
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<OracleControlReconciliationRequestArtifact>, OracleFrameworkError> {
        derive_id(self)
    }

    fn validate(&self) -> Result<(), OracleFrameworkError> {
        require_v1(self.schema_version)?;
        let attempt_id = self.attempt.identity()?;
        let unavailable_or_missing = has_unavailable_or_missing(&self.attempt, &self.evidence);
        let mechanism_failure = self
            .evidence
            .receipts()
            .iter()
            .any(control_requires_reconciliation);
        if self.outcome.attempt() != attempt_id
            || self.evidence.attempt() != attempt_id
            || self.outcome.evidence() != self.evidence.identity()?
            || self
                .outcome
                .claims()
                .iter()
                .all(|claim| claim.status() == OracleClaimAdmissionStatusV1::Admitted)
            || (!unavailable_or_missing && !mechanism_failure)
        {
            return Err(OracleFrameworkError::RevisionRequestInvalid);
        }
        Ok(())
    }
}

fn has_unavailable_or_missing(
    attempt: &OracleAdmissionAttemptV1,
    evidence: &OracleAdmissionEvidenceV1,
) -> bool {
    attempt.required_controls().iter().any(|obligation| {
        evidence
            .receipts()
            .iter()
            .find(|receipt| {
                receipt.item() == obligation.item()
                    && receipt.control() == obligation.control()
                    && receipt.mechanism() == obligation.mechanism()
            })
            .is_none_or(|receipt| receipt.result() == OracleControlResultV1::Unavailable)
    })
}

impl TryFrom<OracleControlReconciliationRequestWire> for OracleControlReconciliationRequestV1 {
    type Error = OracleFrameworkError;

    fn try_from(wire: OracleControlReconciliationRequestWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            attempt: wire.attempt,
            outcome: wire.outcome,
            evidence: wire.evidence,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for OracleControlReconciliationRequestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        OracleControlReconciliationRequestWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

type AdmissionBuckets = (
    Vec<ContentId<OracleItemArtifact>>,
    Vec<ContentId<OracleItemArtifact>>,
    Vec<ContentId<OracleItemArtifact>>,
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
        .items()
        .map(OracleItemV1::identity)
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
        let OracleObligationResolutionV1::Contributed { items, .. } = &entry.resolution else {
            return Err(OracleFrameworkError::AdmissionOutcomeInvalid);
        };
        let buckets = if let Some((_, buckets)) = by_claim
            .iter_mut()
            .find(|(claim, _)| *claim == entry.dimension.claim)
        {
            buckets
        } else {
            let new_index = by_claim.len();
            by_claim.push((entry.dimension.claim, AdmissionBuckets::default()));
            &mut by_claim[new_index].1
        };
        for item in items {
            let item_id = item.identity()?;
            let mut missing = false;
            let mut failed = false;
            for control in policy.required_controls() {
                match receipt_map.get(&(item_id, *control)) {
                    Some(receipt) if receipt.result == OracleControlResultV1::Passed => {}
                    Some(receipt) if receipt.result == OracleControlResultV1::Failed => {
                        if control_requires_revision(receipt) {
                            failed = true;
                        } else {
                            missing = true;
                        }
                    }
                    Some(_) | None => missing = true,
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
    #[error("invalid {0}")]
    InvalidText(&'static str),
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
    #[error("Oracle numerical allowance has no stated provenance")]
    UnjustifiedAllowance,
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
    #[error("oracle dimension plane does not match its concern")]
    DimensionPlaneMismatch,
    #[error("oracle exploration revision overflowed")]
    RevisionOverflow,
    #[error("oracle exploration revision and parent lineage disagree")]
    RevisionLineageMismatch,
    #[error("oracle exploration strategy budget is exhausted")]
    StrategyBudgetExhausted,
    #[error("oracle exploration experiment budget is exhausted")]
    ExperimentBudgetExhausted,
    #[error("strategy is not eligible for the exact dimension")]
    IneligibleStrategy,
    #[error("oracle exploration ledger transition is invalid")]
    InvalidLedgerTransition,
    #[error("oracle strategy run binding changed")]
    StrategyRunBindingMismatch,
    #[error("oracle experiment request binding changed")]
    ExperimentBindingMismatch,
    #[error("oracle dimension is not in the exploration ledger")]
    UnknownDimension,
    #[error("oracle experiment observation is duplicated")]
    DuplicateObservation,
    #[error("oracle experiment observation binding changed")]
    ObservationBindingMismatch,
    #[error("Oracle portfolio element cell or strategy-run binding changed")]
    PortfolioElementBindingMismatch,
    #[error("Oracle item is bound to another Controller-derived dimension")]
    OracleItemBindingMismatch,
    #[error("Oracle item set must be non-empty, unique, and every item must own material")]
    OracleItemSetInvalid,
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
    #[error("duplicate control receipt for one item and family")]
    DuplicateControlReceipt,
    #[error("Oracle admission outcome structure is inconsistent")]
    AdmissionOutcomeInvalid,
    #[error("Oracle Review cannot approve a portfolio with unresolved obligations")]
    ReviewCannotApproveUnresolved,
    #[error("Oracle Review findings must be non-empty, exact, unique dimension issues")]
    ReviewFindingsInvalid,
    #[error("Oracle Review is bound to another portfolio proposal")]
    ReviewProposalMismatch,
    #[error("Oracle Revision request must carry a failed gate's exact feedback lineage")]
    RevisionRequestInvalid,
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
    use super::*;

    fn f32_bytes(values: &[f32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_bits().to_le_bytes())
            .collect()
    }

    fn absolute(allowance: f32) -> OracleCheckAssertionV1 {
        OracleCheckAssertionV1::new(
            OracleComparatorV1::AbsoluteBinary32 {
                allowance: OracleAllowanceBitsV1::new(allowance.to_bits()),
            },
            OracleAllowanceProvenanceV1::MeasuredNoiseFloor,
        )
        .expect("assertion")
    }

    // A tolerance nobody can account for is a number somebody chose, and a judge resting on one
    // cannot say how wrong a candidate would have to be before it complained.
    #[test]
    fn a_tolerance_without_a_stated_origin_is_not_a_pass_condition() {
        assert!(
            OracleCheckAssertionV1::new(
                OracleComparatorV1::AbsoluteBinary32 {
                    allowance: OracleAllowanceBitsV1::new(1.0e-6_f32.to_bits()),
                },
                OracleAllowanceProvenanceV1::NotApplicable,
            )
            .is_err()
        );
        // An exact comparison has nothing to account for, so claiming an origin is also wrong.
        assert!(
            OracleCheckAssertionV1::new(
                OracleComparatorV1::ExactBytes,
                OracleAllowanceProvenanceV1::CallerDeclared,
            )
            .is_err()
        );
        // Neither is a negative or non-finite tolerance, which accepts everything or nothing.
        assert!(
            OracleCheckAssertionV1::new(
                OracleComparatorV1::AbsoluteBinary32 {
                    allowance: OracleAllowanceBitsV1::new((-1.0_f32).to_bits()),
                },
                OracleAllowanceProvenanceV1::MeasuredNoiseFloor,
            )
            .is_err()
        );
    }

    // An implementation that stops early has produced the wrong answer, not an unreadable one.
    // Reporting that as uncomparable would let it through by producing too little to judge.
    #[test]
    fn an_observation_of_the_wrong_length_is_rejected_rather_than_excused() {
        let reference = f32_bytes(&[1.0, 2.0, 3.0]);
        let truncated = f32_bytes(&[1.0, 2.0]);

        assert_eq!(
            evaluate_check_assertion(absolute(1.0e-6), &reference, &truncated),
            OracleAssertionOutcomeV1::Rejected
        );
        // A reference that is not a whole binary32 array cannot state a requirement at all.
        assert_eq!(
            evaluate_check_assertion(absolute(1.0e-6), &reference[..5], &reference[..5]),
            OracleAssertionOutcomeV1::Uncomparable
        );
    }

    // Two identical infinities are the same value. Subtracting them yields NaN, which compares
    // false against every tolerance, so a naive difference would reject a correct result.
    #[test]
    fn matching_non_finite_values_are_accepted_and_differing_ones_are_not() {
        assert_eq!(
            evaluate_check_assertion(
                absolute(1.0e-6),
                &f32_bytes(&[f32::INFINITY]),
                &f32_bytes(&[f32::INFINITY])
            ),
            OracleAssertionOutcomeV1::Accepted
        );
        assert_eq!(
            evaluate_check_assertion(
                absolute(1.0e9),
                &f32_bytes(&[f32::INFINITY]),
                &f32_bytes(&[f32::NEG_INFINITY])
            ),
            OracleAssertionOutcomeV1::Rejected
        );
        // A tolerance can never make a NaN acceptable where a finite value was required.
        assert_eq!(
            evaluate_check_assertion(absolute(1.0e9), &f32_bytes(&[1.0]), &f32_bytes(&[f32::NAN])),
            OracleAssertionOutcomeV1::Rejected
        );
    }

    #[test]
    fn calibration_separates_a_judge_that_cannot_pass_from_one_that_cannot_fail() {
        let reference = f32_bytes(&[1.0, 2.0, 3.0]);
        let wrong = vec![f32_bytes(&[1.0, 2.0, 4.0])];

        assert_eq!(
            calibrate_check_assertion(absolute(1.0e-6), &reference, &wrong),
            OracleCalibrationOutcomeV1::Calibrated
        );
        // Wide enough to accept the known-wrong variant: it can no longer fail.
        assert_eq!(
            calibrate_check_assertion(absolute(10.0), &reference, &wrong),
            OracleCalibrationOutcomeV1::FailedSensitivity
        );
        // Offering no wrong variants tests nothing, which is not the same as passing.
        assert_eq!(
            calibrate_check_assertion(absolute(1.0e-6), &reference, &[]),
            OracleCalibrationOutcomeV1::NoNegativeVariants
        );
    }

    fn item_plan(item: &OracleItemV1, objective: &str) -> OracleCheckPlanV1 {
        OracleCheckPlanV1::new(
            item.identity().expect("item identity"),
            OracleCheckMethodV1::StaticAnalysis,
            OracleCheckObjective::new(objective).expect("objective"),
            OracleCheckSetup::new("Inspect the exact task-local implementation.").expect("setup"),
            OracleCheckObservation::new("Record the implementation property.")
                .expect("observation"),
            OracleCheckPassCondition::new("The property matches the admitted contract.")
                .expect("pass condition"),
            OracleCheckAssertionV1::new(
                OracleComparatorV1::ExactBytes,
                OracleAllowanceProvenanceV1::NotApplicable,
            )
            .expect("assertion"),
            vec![OracleCheckEvidenceV1::AdmittedIntent {
                contract: ContentId::derive(b"admitted-intent").expect("intent identity"),
            }],
        )
        .expect("plan")
    }

    #[test]
    #[allow(
        clippy::similar_names,
        clippy::too_many_lines,
        reason = "sibling and revision labels keep the exact lineage matrix legible"
    )]
    fn item_revision_and_review_lineage_cannot_cross_siblings_or_revisions() {
        let dimension = OracleDimensionV1::new(
            ContentId::derive(b"claim").expect("claim identity"),
            OracleConcernV1::ObservableOutputs,
            OracleStrategyRoleV1::Synthesis,
        );
        let dimension_id = dimension.identity().expect("dimension identity");
        let item_a = OracleItemV1::new(
            dimension_id,
            OracleItemStatement::new("Validate the primary result.").expect("statement"),
        )
        .expect("item");
        let item_b = OracleItemV1::new(
            dimension_id,
            OracleItemStatement::new("Validate the boundary behavior.").expect("statement"),
        )
        .expect("item");
        let draft_a1 = OracleItemDraftV1::initial(
            item_a.clone(),
            ContentId::derive(b"run-a1").expect("run identity"),
            vec![item_plan(&item_a, "Establish the primary result.")],
        )
        .expect("initial draft A");
        let draft_b1 = OracleItemDraftV1::initial(
            item_b.clone(),
            ContentId::derive(b"run-b1").expect("run identity"),
            vec![item_plan(&item_b, "Establish the boundary behavior.")],
        )
        .expect("initial draft B");
        let item_a_id = item_a.identity().expect("item A identity");
        let feedback_a1 = OracleItemReviewV1::needs_revision(
            &draft_a1,
            vec![
                OracleReviewFindingV1::new(
                    item_a_id,
                    OracleReviewIssueClassV1::SetupIncomplete,
                    OracleReviewExplanation::new("The setup omits one required input.")
                        .expect("explanation"),
                    OracleReviewRequiredChange::new("Specify every required input.")
                        .expect("required change"),
                ),
                OracleReviewFindingV1::new(
                    item_a_id,
                    OracleReviewIssueClassV1::SetupIncomplete,
                    OracleReviewExplanation::new("The setup omits the launch shape.")
                        .expect("explanation"),
                    OracleReviewRequiredChange::new("Specify the launch shape.")
                        .expect("required change"),
                ),
            ],
        )
        .expect("review feedback");

        assert!(matches!(
            feedback_a1.validate_against(&draft_b1),
            Err(OracleFrameworkError::ReviewProposalMismatch)
        ));

        let draft_a2 = OracleItemDraftV1::revise(
            &draft_a1,
            ContentId::derive(b"run-a2").expect("run identity"),
            vec![item_plan(
                &item_a,
                "Establish the corrected primary result.",
            )],
        )
        .expect("revised draft A");
        assert_eq!(
            draft_a2.parent(),
            Some(draft_a1.identity().expect("draft A1 identity"))
        );
        assert_eq!(draft_a2.revision().get(), 2);
        assert!(matches!(
            feedback_a1.validate_against(&draft_a2),
            Err(OracleFrameworkError::ReviewProposalMismatch)
        ));

        let accepted_a = OracleAcceptedItemV1::new(
            &draft_a2,
            &OracleItemReviewV1::approved(&draft_a2).expect("approval A"),
        )
        .expect("accepted A");
        let accepted_b = OracleAcceptedItemV1::new(
            &draft_b1,
            &OracleItemReviewV1::approved(&draft_b1).expect("approval B"),
        )
        .expect("accepted B");
        assert_eq!(accepted_a.draft(), &draft_a2);
        assert_eq!(accepted_b.draft(), &draft_b1);

        let workspace = OracleWorkspaceV1::new(&OracleWorkspaceInput {
            task_id: TaskId::new(),
            admitted_intent: id("admitted intent"),
            sir_input: id("sir input"),
            sir_task_bundle: id("sir task bundle"),
            source: id("source"),
            documentation: id("documentation"),
            build_and_tests: id("build and tests"),
            knowledge: id("knowledge"),
            research_tools: id("research tools"),
            experiment_tools: id("experiment tools"),
            capability_grant: id("capability grant"),
            coverage_policy: id("coverage policy"),
            strategy_catalog: id("strategy catalog"),
            budget: OracleExplorationBudgetV1 {
                strategy_runs: OracleStrategyRunLimit::new(2).expect("run limit"),
                experiments: OracleExperimentLimit::new(1).expect("experiment limit"),
                item_discovery_revisions: OracleItemDiscoveryRevisionLimit::new(4)
                    .expect("item discovery revision limit"),
                item_revisions: OracleItemRevisionLimit::new(4).expect("item revision limit"),
            },
        });
        let proposal = OraclePortfolioProposalV1::assemble(
            &workspace,
            vec![dimension],
            vec![accepted_a, accepted_b],
        )
        .expect("reviewed portfolio");
        assert_eq!(proposal.accepted_items().len(), 2);
        assert_eq!(proposal.elements().len(), 2);
        let persisted = serde_json::to_value(&proposal).expect("proposal json");
        let restored: OraclePortfolioProposalV1 =
            serde_json::from_value(persisted).expect("reviewed proposal roundtrip");
        assert_eq!(restored, proposal);
    }

    fn id<A: ContentType>(label: &str) -> ContentId<A> {
        ContentId::derive(label.as_bytes()).expect("id")
    }

    fn claim(label: &str) -> ContentId<OracleClaimArtifact> {
        id(label)
    }

    fn oracle_item(dimension: &OracleDimensionV1, statement: &str) -> OracleItemV1 {
        OracleItemV1::new(
            dimension.identity().expect("dimension id"),
            OracleItemStatement::new(statement).expect("item statement"),
        )
        .expect("Oracle item")
    }

    fn test_workspace() -> OracleWorkspaceV1 {
        OracleWorkspaceV1::new(&OracleWorkspaceInput {
            task_id: TaskId::new(),
            admitted_intent: id("admitted-intent"),
            sir_input: id("sir-input"),
            sir_task_bundle: id("sir-task-bundle"),
            source: id("source"),
            documentation: id("documentation"),
            build_and_tests: id("build-and-tests"),
            knowledge: id("knowledge"),
            research_tools: id("research-tools"),
            experiment_tools: id("experiment-tools"),
            capability_grant: id("capability-grant"),
            coverage_policy: id("coverage-policy"),
            strategy_catalog: id("strategy-catalog"),
            budget: OracleExplorationBudgetV1 {
                strategy_runs: OracleStrategyRunLimit::new(8).expect("run limit"),
                experiments: OracleExperimentLimit::new(2).expect("experiment limit"),
                item_discovery_revisions: OracleItemDiscoveryRevisionLimit::new(4)
                    .expect("item discovery revision limit"),
                item_revisions: OracleItemRevisionLimit::new(4).expect("item revision limit"),
            },
        })
    }

    fn accepted_item(item: &OracleItemV1, run_label: &str) -> OracleAcceptedItemV1 {
        let draft = OracleItemDraftV1::initial(
            item.clone(),
            id(run_label),
            vec![item_plan(item, "Establish the exact item obligation.")],
        )
        .expect("item draft");
        let review = OracleItemReviewV1::approved(&draft).expect("item approval");
        OracleAcceptedItemV1::new(&draft, &review).expect("accepted item")
    }

    fn reviewed_proposal(
        dimension: OracleDimensionV1,
        items: Vec<OracleItemV1>,
    ) -> OraclePortfolioProposalV1 {
        let accepted = items
            .into_iter()
            .enumerate()
            .map(|(index, item)| accepted_item(&item, &format!("run-{index}")))
            .collect();
        OraclePortfolioProposalV1::assemble(&test_workspace(), vec![dimension], accepted)
            .expect("reviewed proposal")
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
        let items = derive_oracle_dimensions(&claims, &policy).expect("items");

        assert_eq!(items.len(), claims.len() * policy.concerns().len() * 2);
        for claim in claims {
            for concern in policy.concerns() {
                assert!(items.contains(&OracleDimensionV1::new(
                    claim,
                    *concern,
                    OracleStrategyRoleV1::Synthesis
                )));
                assert!(items.contains(&OracleDimensionV1::new(
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
    fn persisted_workspace_cannot_disable_the_item_revision_bound() {
        let mut json = serde_json::to_value(test_workspace()).expect("workspace json");
        json["budget"]["item_revisions"] = serde_json::json!(0);
        assert!(serde_json::from_value::<OracleWorkspaceV1>(json).is_err());
    }

    #[test]
    fn exploration_cannot_open_when_any_dimension_has_no_strategy() {
        let policy = OracleCoveragePolicyV1::new(
            OracleCoverageProfileV1::Correctness,
            OracleAdversarialPolicyV1::RequiredForEveryConcern,
        );
        let items = derive_oracle_dimensions(&[claim("claim-a")], &policy).expect("items");
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
    fn one_dimension_can_publish_many_items_and_controls_expand_per_item() {
        let dimension = OracleDimensionV1::new(
            claim("claim-a"),
            OracleConcernV1::BoundaryAndDegenerateInputs,
            OracleStrategyRoleV1::Synthesis,
        );
        let mut items = vec![
            oracle_item(&dimension, "nominal-domain behavior"),
            oracle_item(&dimension, "boundary-domain behavior"),
        ];
        items.sort();
        let proposal = reviewed_proposal(dimension, items.clone());
        assert_eq!(proposal.items().count(), 2);
        assert_eq!(proposal.accepted_items().len(), 2);

        let (policy, _, attempt) = admission_context(&proposal);
        assert_eq!(
            attempt.required_controls().len(),
            items.len() * policy.required_controls().len()
        );
        let controlled_items = attempt
            .required_controls()
            .iter()
            .map(OracleControlObligationV1::item)
            .collect::<HashSet<_>>();
        assert_eq!(controlled_items.len(), 2);
        assert!(
            items
                .iter()
                .all(|item| controlled_items.contains(&item.identity().expect("item identity")))
        );
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
        let items = derive_oracle_dimensions(&[claim("claim-a")], &policy).expect("items");
        let catalog = OracleStrategyCatalogV1::new(vec![all_concerns_registration(
            OracleStrategyRoleV1::Synthesis,
            "deterministic-synthesis",
        )])
        .expect("catalog");
        let budget = OracleExplorationBudgetV1 {
            strategy_runs: OracleStrategyRunLimit::new(32).expect("run limit"),
            experiments: OracleExperimentLimit::new(8).expect("experiment limit"),
            item_discovery_revisions: OracleItemDiscoveryRevisionLimit::new(4)
                .expect("item discovery revision limit"),
            item_revisions: OracleItemRevisionLimit::new(4).expect("item revision limit"),
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
        let dimension = ledger.entries()[0].dimension.clone();
        let item = dimension.identity().expect("item");
        let strategy = OracleStrategyName::new("deterministic-synthesis").expect("strategy");
        let run = OracleStrategyRunV1::new(workspace_id, &dimension, strategy, &catalog)
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
        let proposed_item = oracle_item(&dimension, "reference probe result is preserved");
        let element = OraclePortfolioElementV1::new(
            proposed_item.identity().expect("Oracle item id"),
            run_id,
            OraclePortfolioElementKindV1::Reference(id("reference")),
            vec![observation_id],
        )
        .expect("element");
        let contributed = resumed
            .record_contribution(item, run_id, &[proposed_item], &[element])
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
    fn persisted_dimension_cannot_move_a_concern_to_another_plane() {
        let item = OracleDimensionV1::new(
            claim("claim-a"),
            OracleConcernV1::ObservableOutputs,
            OracleStrategyRoleV1::Synthesis,
        );
        let mut json = serde_json::to_value(item).expect("json");
        json["plane"] = serde_json::json!("input-domain");
        assert!(serde_json::from_value::<OracleDimensionV1>(json).is_err());
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
        let item = OracleDimensionV1::new(
            claim_body.identity().expect("claim id"),
            OracleConcernV1::ObservableOutputs,
            OracleStrategyRoleV1::Synthesis,
        );
        let proposed_item = oracle_item(&item, "reference semantics preserve observable output");
        let item_id = proposed_item.identity().expect("item id");
        let plan = item_plan(&proposed_item, "Establish the observable output semantics.");
        let material_bytes = cairn_codec::to_vec(&plan).expect("check plan bytes");
        let draft =
            OracleItemDraftV1::initial(proposed_item, id("run"), vec![plan]).expect("item draft");
        let review = OracleItemReviewV1::approved(&draft).expect("item approval");
        let accepted = OracleAcceptedItemV1::new(&draft, &review).expect("accepted item");
        let proposal = OraclePortfolioProposalV1::assemble(
            &test_workspace(),
            vec![item.clone()],
            vec![accepted],
        )
        .expect("reviewed proposal");
        let element = proposal.elements()[0].clone();
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
        assert!(matches!(
            OracleRevisionRequestV1::from_admission(
                attempt.clone(),
                partial.clone(),
                no_evidence.clone(),
            ),
            Err(OracleFrameworkError::RevisionRequestInvalid)
        ));
        OracleControlReconciliationRequestV1::from_admission(
            attempt.clone(),
            partial.clone(),
            no_evidence.clone(),
        )
        .expect("missing infrastructure evidence requires reconciliation");
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
            None,
        )
        .expect("control receipt");
        assert!(matches!(
            OracleAdmissionEvidenceV1::new(&attempt, vec![unknown_item_receipt]),
            Err(OracleFrameworkError::AdmissionEvidenceBindingMismatch)
        ));

        let reused_receipt = id("reused trusted receipt");
        let duplicate_provenance = vec![
            OracleControlReceiptV1::new(
                proposal.identity().expect("proposal id"),
                item_id,
                OracleControlFamilyV1::Honest,
                mechanisms
                    .mechanism(OracleControlFamilyV1::Honest)
                    .expect("honest mechanism"),
                reused_receipt,
                OracleControlResultV1::Passed,
                None,
            )
            .expect("control receipt"),
            OracleControlReceiptV1::new(
                proposal.identity().expect("proposal id"),
                item_id,
                OracleControlFamilyV1::Mutant,
                mechanisms
                    .mechanism(OracleControlFamilyV1::Mutant)
                    .expect("mutant mechanism"),
                reused_receipt,
                OracleControlResultV1::Passed,
                None,
            )
            .expect("control receipt"),
        ];
        assert!(matches!(
            OracleAdmissionEvidenceV1::new(&attempt, duplicate_provenance),
            Err(OracleFrameworkError::DuplicateControlReceipt)
        ));

        let failed = OracleControlReceiptV1::new(
            proposal.identity().expect("proposal id"),
            item_id,
            OracleControlFamilyV1::Mutant,
            mechanisms
                .mechanism(OracleControlFamilyV1::Mutant)
                .expect("mutant mechanism"),
            id("receipt"),
            OracleControlResultV1::Failed,
            Some(OracleControlDiagnosticV1::new(
                OracleControlFailureClassV1::NegativeChallengeAccepted,
                OracleControlDiagnosticSummary::new("mutant control accepted the challenged plan")
                    .expect("diagnostic summary"),
                id("stdout"),
                id("stderr"),
            )),
        )
        .expect("control receipt");
        let mut forged_failed_receipt = serde_json::to_value(&failed).expect("failed receipt json");
        forged_failed_receipt["diagnostic"] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<OracleControlReceiptV1>(forged_failed_receipt).is_err(),
            "persisted failed receipts cannot bypass diagnostic invariants"
        );
        let mut forged_failure_owner = serde_json::to_value(&failed).expect("failed receipt json");
        forged_failure_owner["diagnostic"]["failure_class"] =
            serde_json::json!("oracle-artifact-rejected");
        assert!(
            serde_json::from_value::<OracleControlReceiptV1>(forged_failure_owner).is_err(),
            "a negative control cannot be persisted as an artifact-owned failure"
        );
        let mut negative_receipts = vec![failed];
        negative_receipts.extend(
            policy
                .required_controls()
                .iter()
                .filter(|control| **control != OracleControlFamilyV1::Mutant)
                .map(|control| {
                    OracleControlReceiptV1::new(
                        proposal.identity().expect("proposal id"),
                        item_id,
                        *control,
                        mechanisms.mechanism(*control).expect("qualified mechanism"),
                        id(&format!("negative-control companion {control:?}")),
                        OracleControlResultV1::Passed,
                        None,
                    )
                    .expect("passed companion receipt")
                }),
        );
        let failed_evidence =
            OracleAdmissionEvidenceV1::new(&attempt, negative_receipts).expect("failed evidence");
        let unresolved =
            recompute_oracle_admission(&proposal, &policy, &mechanisms, &attempt, &failed_evidence)
                .expect("unresolved mechanism failure");
        assert_eq!(
            unresolved.claims[0].status,
            OracleClaimAdmissionStatusV1::Partial
        );
        assert!(matches!(
            OracleRevisionRequestV1::from_admission(
                attempt.clone(),
                unresolved.clone(),
                failed_evidence.clone(),
            ),
            Err(OracleFrameworkError::RevisionRequestInvalid)
        ));
        OracleControlReconciliationRequestV1::from_admission(
            attempt.clone(),
            unresolved,
            failed_evidence.clone(),
        )
        .expect("negative challenge acceptance requires mechanism reconciliation");

        let artifact_failure = OracleControlReceiptV1::new(
            proposal.identity().expect("proposal id"),
            item_id,
            OracleControlFamilyV1::Honest,
            mechanisms
                .mechanism(OracleControlFamilyV1::Honest)
                .expect("honest mechanism"),
            id("artifact-failure-receipt"),
            OracleControlResultV1::Failed,
            Some(OracleControlDiagnosticV1::new(
                OracleControlFailureClassV1::OracleArtifactRejected,
                OracleControlDiagnosticSummary::new("honest control rejected the plan")
                    .expect("diagnostic summary"),
                id("artifact stdout"),
                id("artifact stderr"),
            )),
        )
        .expect("artifact failure");
        let mut artifact_receipts = vec![artifact_failure];
        artifact_receipts.extend(
            policy
                .required_controls()
                .iter()
                .filter(|control| **control != OracleControlFamilyV1::Honest)
                .map(|control| {
                    OracleControlReceiptV1::new(
                        proposal.identity().expect("proposal id"),
                        item_id,
                        *control,
                        mechanisms.mechanism(*control).expect("qualified mechanism"),
                        id(&format!("artifact-control companion {control:?}")),
                        OracleControlResultV1::Passed,
                        None,
                    )
                    .expect("passed companion receipt")
                }),
        );
        let artifact_evidence =
            OracleAdmissionEvidenceV1::new(&attempt, artifact_receipts).expect("artifact evidence");
        let rejected = recompute_oracle_admission(
            &proposal,
            &policy,
            &mechanisms,
            &attempt,
            &artifact_evidence,
        )
        .expect("artifact rejected");
        assert_eq!(
            rejected.claims[0].status,
            OracleClaimAdmissionStatusV1::Rejected
        );
        OracleRevisionRequestV1::from_admission(
            attempt.clone(),
            rejected.clone(),
            artifact_evidence.clone(),
        )
        .expect("artifact control failure requires item revision");
        assert!(matches!(
            OracleControlReconciliationRequestV1::from_admission(
                attempt.clone(),
                rejected.clone(),
                artifact_evidence,
            ),
            Err(OracleFrameworkError::RevisionRequestInvalid)
        ));

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
                    item_id,
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
                    None,
                )
                .expect("control receipt")
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
}

/// Evaluates one check assertion against a reference observation and a candidate observation.
///
/// This is the whole of what makes a mechanism able to judge rather than merely to inspect. It
/// takes two observations and says whether the candidate's is acceptable under the plan's stated
/// comparator, and it is deliberately the only place that decision is made, so a runner cannot
/// invent a looser rule than the plan declared.
///
/// Both observations are raw bytes because that is what an execution produces. A binary32
/// comparator therefore requires both to be whole binary32 arrays of equal length: a length
/// mismatch is a failure rather than a shorter comparison, since comparing a prefix would accept a
/// candidate that stopped early.
///
/// This is deliberately not re-exported yet. Its consumer is the control runner that will judge a
/// real candidate observation, and until that exists the calibration protocol below is the only
/// caller; exporting it earlier would publish a capability with nobody behind it.
#[must_use]
pub fn evaluate_check_assertion(
    assertion: OracleCheckAssertionV1,
    reference: &[u8],
    candidate: &[u8],
) -> OracleAssertionOutcomeV1 {
    match assertion.comparator() {
        OracleComparatorV1::ExactBytes => {
            if reference == candidate {
                OracleAssertionOutcomeV1::Accepted
            } else {
                OracleAssertionOutcomeV1::Rejected
            }
        }
        OracleComparatorV1::AbsoluteBinary32 { allowance } => {
            compare_binary32(reference, candidate, allowance, false)
        }
        OracleComparatorV1::RelativeBinary32 { allowance } => {
            compare_binary32(reference, candidate, allowance, true)
        }
    }
}

/// What one assertion concluded about one candidate observation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleAssertionOutcomeV1 {
    Accepted,
    Rejected,
    /// The observations cannot be compared under this comparator at all.
    ///
    /// This is distinct from rejection: a truncated or misshapen observation says nothing about
    /// whether the candidate is correct, and reporting it as a rejection would attribute a defect
    /// to the candidate that the evidence does not support.
    Uncomparable,
}

fn compare_binary32(
    reference: &[u8],
    candidate: &[u8],
    allowance: OracleAllowanceBitsV1,
    relative: bool,
) -> OracleAssertionOutcomeV1 {
    const WIDTH: usize = 4;
    // Only the reference decides whether a comparison can be stated at all. If it is not a whole
    // binary32 array, or the tolerance is not a tolerance, nothing here can say what was required.
    if reference.is_empty() || reference.len() % WIDTH != 0 || !allowance.is_usable() {
        return OracleAssertionOutcomeV1::Uncomparable;
    }
    // A candidate of a different length is wrong, not unreadable: it did not produce the values
    // that were required. Calling that uncomparable would let an implementation that stops early
    // pass by producing too little to judge.
    if reference.len() != candidate.len() {
        return OracleAssertionOutcomeV1::Rejected;
    }
    let tolerance = f32::from_bits(allowance.get());
    for (expected, actual) in reference
        .chunks_exact(WIDTH)
        .zip(candidate.chunks_exact(WIDTH))
    {
        let expected = f32::from_bits(u32::from_le_bytes([
            expected[0],
            expected[1],
            expected[2],
            expected[3],
        ]));
        let actual = f32::from_bits(u32::from_le_bytes([
            actual[0], actual[1], actual[2], actual[3],
        ]));
        // A non-finite pair is only acceptable when both sides agree on which non-finite value it
        // is. Subtracting them would produce NaN and compare false against every tolerance, which
        // would reject two identical infinities.
        if !expected.is_finite() || !actual.is_finite() {
            if expected.to_bits() != actual.to_bits() {
                return OracleAssertionOutcomeV1::Rejected;
            }
            continue;
        }
        let difference = (expected - actual).abs();
        let bound = if relative {
            tolerance * expected.abs()
        } else {
            tolerance
        };
        // Incomparability here is a rejection, not an excuse: a difference that cannot be
        // ordered against the bound is one the tolerance does not cover.
        if !matches!(
            difference.partial_cmp(&bound),
            Some(core::cmp::Ordering::Less | core::cmp::Ordering::Equal)
        ) {
            return OracleAssertionOutcomeV1::Rejected;
        }
    }
    OracleAssertionOutcomeV1::Accepted
}

/// What a mechanism's self-certification found before it was allowed to judge anything.
///
/// A judge that cannot fail is not a judge, so a mechanism proves it can discriminate before it is
/// used. The protocol is deliberately not a single "it worked" bit: specificity and sensitivity
/// fail in different ways, and a floor taken from a comparison that produced no difference at all
/// is a zero wearing a floor's clothes, which turns the judge into a machine that rejects
/// everything.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleCalibrationOutcomeV1 {
    /// The mechanism accepted the reference against itself and rejected every known-wrong variant.
    Calibrated,
    /// The mechanism rejected the reference against itself, so it would reject correct work.
    FailedSpecificity,
    /// Some known-wrong variant was accepted, so the mechanism cannot detect that class of defect.
    FailedSensitivity,
    /// A variant could not be compared at all, so the calibration proved nothing either way.
    Uncomparable,
    /// No known-wrong variants were offered, so nothing tested whether the judge can fail.
    NoNegativeVariants,
}

/// Runs the calibration protocol for one assertion against a reference and its known-wrong variants.
///
/// Specificity is checked against the reference compared with itself, which is the one comparison
/// a correct implementation must always survive. Sensitivity is checked against variants that are
/// known to be wrong; every one of them has to be rejected, because a judge that misses one class
/// of defect will stay silent about it for every candidate afterwards.
#[must_use]
pub fn calibrate_check_assertion(
    assertion: OracleCheckAssertionV1,
    reference: &[u8],
    wrong_variants: &[Vec<u8>],
) -> OracleCalibrationOutcomeV1 {
    if wrong_variants.is_empty() {
        return OracleCalibrationOutcomeV1::NoNegativeVariants;
    }
    match evaluate_check_assertion(assertion, reference, reference) {
        OracleAssertionOutcomeV1::Accepted => {}
        OracleAssertionOutcomeV1::Rejected => {
            return OracleCalibrationOutcomeV1::FailedSpecificity;
        }
        OracleAssertionOutcomeV1::Uncomparable => {
            return OracleCalibrationOutcomeV1::Uncomparable;
        }
    }
    for variant in wrong_variants {
        match evaluate_check_assertion(assertion, reference, variant) {
            OracleAssertionOutcomeV1::Rejected => {}
            OracleAssertionOutcomeV1::Accepted => {
                return OracleCalibrationOutcomeV1::FailedSensitivity;
            }
            OracleAssertionOutcomeV1::Uncomparable => {
                return OracleCalibrationOutcomeV1::Uncomparable;
            }
        }
    }
    OracleCalibrationOutcomeV1::Calibrated
}
