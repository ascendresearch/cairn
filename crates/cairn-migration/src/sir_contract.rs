//! Strong current-V1 input and proposal contracts for Semantic Intent Recovery.

use std::collections::HashSet;

use cairn_execution::TargetEnvironmentName;
use cairn_protocol::{ContentId, ContentType, EpisodeId, TaskId};
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    ArgumentIndex, DataType, EntryPointName,
    sir::{
        SirError, SirIntentHypothesisSetProposalArtifact, SirSourceCitationV1,
        SirTaskBundleArtifact, SirTaskLimits,
    },
};

#[cfg(feature = "agent-runtime")]
use crate::sir::SirTaskWorkspace;

const SCHEMA_V1: u16 = 1;
const MAX_ARGUMENTS: usize = 64;
const MAX_CALLER_CLAIMS: usize = 32;
const MAX_DECLARATIONS: usize = 32;
const MAX_AUTHORIZED_EVIDENCE: usize = 64;
const MAX_OBSERVATIONS: usize = 64;
const MAX_HYPOTHESES: usize = 16;
const MAX_CONFLICTS: usize = 16;
const MAX_UNKNOWNS: usize = 32;
const MAX_INVARIANTS: usize = 32;
const MAX_FREEDOMS: usize = 32;
const MAX_DISPOSITIONS: usize = 32;
const MAX_EXPERIMENTS: usize = 32;
const MAX_EDGES: usize = 32;
const MAX_CITATIONS: usize = 8;

macro_rules! bounded_text_type {
    ($(#[$meta:meta])* $name:ident, $kind:literal, $max:expr) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates validated semantic text.
            ///
            /// # Errors
            ///
            /// Rejects empty, oversized, untrimmed, or control-containing text.
            pub fn new(value: impl Into<String>) -> Result<Self, SirError> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > $max
                    || value.trim() != value
                    || value.chars().any(char::is_control)
                {
                    return Err(SirError::InvalidValue($kind));
                }
                Ok(Self(value))
            }

            /// Returns validated text.
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

macro_rules! local_id_type {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a bounded task-local semantic identity.
            ///
            /// # Errors
            ///
            /// Rejects values outside the lowercase ASCII selector grammar.
            pub fn new(value: impl Into<String>) -> Result<Self, SirError> {
                let value = value.into();
                let valid_edge = |byte: Option<&u8>| {
                    byte.is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
                };
                if value.is_empty()
                    || value.len() > 64
                    || !valid_edge(value.as_bytes().first())
                    || !valid_edge(value.as_bytes().last())
                    || !value.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
                {
                    return Err(SirError::InvalidValue($kind));
                }
                Ok(Self(value))
            }

            /// Returns the task-local identity.
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

bounded_text_type!(SirArgumentName, "caller argument name", 128);
bounded_text_type!(SirShapeExpression, "caller shape expression", 512);
bounded_text_type!(SirValueDomainDeclaration, "caller value domain", 1_000);
bounded_text_type!(SirErrorBehaviorDeclaration, "caller error behavior", 1_000);
bounded_text_type!(SirCallerClaimStatement, "caller intent claim", 2_000);
bounded_text_type!(SirExclusionStatement, "caller exclusion", 1_000);
bounded_text_type!(SirDeclaredUnknownQuestion, "caller unknown", 1_000);
bounded_text_type!(SirTargetSoc, "target SoC", 128);
bounded_text_type!(SirTargetToolchain, "target toolchain", 256);
bounded_text_type!(SirObservationStatement, "observed fact", 2_000);
bounded_text_type!(SirHypothesisClaim, "intent hypothesis claim", 2_000);
bounded_text_type!(SirIntentDomain, "intent hypothesis domain", 1_000);
bounded_text_type!(SirConflictStatement, "intent conflict", 2_000);
bounded_text_type!(SirUnknownQuestion, "intent unknown", 2_000);
bounded_text_type!(SirInvariantStatement, "semantic invariant", 2_000);
bounded_text_type!(
    SirOptimizationFreedomStatement,
    "optimization freedom",
    2_000
);
bounded_text_type!(
    SirDispositionRationale,
    "source disposition rationale",
    2_000
);
bounded_text_type!(SirExperimentPlan, "disambiguation experiment plan", 2_000);
bounded_text_type!(SirExperimentPrediction, "disambiguation prediction", 1_000);

local_id_type!(
    /// Caller-authoritative claim identity.
    SirCallerClaimId,
    "caller claim identity"
);
local_id_type!(SirCallerExclusionId, "caller exclusion identity");
local_id_type!(SirDeclaredUnknownId, "caller unknown identity");
local_id_type!(
    /// Source-observation identity, which cannot be substituted for a hypothesis.
    ///
    /// ```compile_fail
    /// use cairn_migration::{SirHypothesisId, SirObservationId};
    /// fn require_hypothesis(_: SirHypothesisId) {}
    /// let observed = SirObservationId::new("source-fact").unwrap();
    /// require_hypothesis(observed);
    /// ```
    SirObservationId,
    "observation identity"
);
local_id_type!(SirHypothesisId, "hypothesis identity");
local_id_type!(SirConflictId, "conflict identity");
local_id_type!(SirUnknownId, "unknown identity");
local_id_type!(SirInvariantId, "invariant identity");
local_id_type!(SirOptimizationFreedomId, "optimization freedom identity");
local_id_type!(SirSourceDispositionId, "source disposition identity");
local_id_type!(SirExperimentId, "experiment identity");

/// Archived caller-provided reference selected for SIR input.
pub enum SirCallerReferenceArtifact {}

impl ContentType for SirCallerReferenceArtifact {
    const DOMAIN: &'static str = "migration.sir-caller-reference.v1";
}

/// Archived prior feedback explicitly allowed into one recovery run.
pub enum SirPriorFeedbackArtifact {}

impl ContentType for SirPriorFeedbackArtifact {
    const DOMAIN: &'static str = "migration.sir-prior-feedback.v1";
}

/// Additional immutable evidence explicitly allowed into one recovery run.
pub enum SirAuthorizedEvidenceArtifact {}

impl ContentType for SirAuthorizedEvidenceArtifact {
    const DOMAIN: &'static str = "migration.sir-authorized-evidence.v1";
}

/// Frozen complete recovery input.
pub enum IntentRecoveryInputArtifact {}

impl ContentType for IntentRecoveryInputArtifact {
    const DOMAIN: &'static str = "migration.intent-recovery-input.v1";
}

/// Exact agent-owned resolved runtime-model artifact cited by a SIR proposal.
///
/// The semantic content domain remains the agent domain; this local marker lets model-free
/// consumers validate the typed reference without linking the agent runtime.
pub enum SirResolvedRuntimeModelArtifact {}

impl ContentType for SirResolvedRuntimeModelArtifact {
    const DOMAIN: &'static str = "agent.resolved-runtime-model.v1";
}

/// Caller-declared ABI role. Runtime handles are not scalar values.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SirCallerArgumentRole {
    InputBuffer,
    OutputBuffer,
    InputOutputBuffer,
    Scalar,
    RuntimeHandle,
}

/// Caller-declared shape without pretending an unknown shape is known.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SirDeclaredShapeV1 {
    Scalar,
    Ranked { dimensions: Vec<SirShapeExpression> },
    UnknownRank,
    OpaqueHandle,
}

/// One machine-readable argument in the caller's minimum declaration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SirCallerArgumentV1 {
    index: ArgumentIndex,
    name: SirArgumentName,
    role: SirCallerArgumentRole,
    data_type: Option<DataType>,
    shape: SirDeclaredShapeV1,
    valid_domain: Option<SirValueDomainDeclaration>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SirCallerArgumentWire {
    index: ArgumentIndex,
    name: SirArgumentName,
    role: SirCallerArgumentRole,
    data_type: Option<DataType>,
    shape: SirDeclaredShapeV1,
    valid_domain: Option<SirValueDomainDeclaration>,
}

impl SirCallerArgumentV1 {
    /// Creates one validated caller argument.
    ///
    /// # Errors
    ///
    /// Rejects a role, data type, and shape combination that is semantically inconsistent.
    pub fn new(
        index: ArgumentIndex,
        name: SirArgumentName,
        role: SirCallerArgumentRole,
        data_type: Option<DataType>,
        shape: SirDeclaredShapeV1,
        valid_domain: Option<SirValueDomainDeclaration>,
    ) -> Result<Self, SirError> {
        let value = Self {
            index,
            name,
            role,
            data_type,
            shape,
            valid_domain,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), SirError> {
        match (self.role, self.data_type, &self.shape) {
            (SirCallerArgumentRole::RuntimeHandle, None, SirDeclaredShapeV1::OpaqueHandle)
            | (SirCallerArgumentRole::Scalar, Some(_), SirDeclaredShapeV1::Scalar)
            | (
                SirCallerArgumentRole::InputBuffer
                | SirCallerArgumentRole::OutputBuffer
                | SirCallerArgumentRole::InputOutputBuffer,
                Some(_),
                SirDeclaredShapeV1::UnknownRank,
            ) => {}
            (
                SirCallerArgumentRole::InputBuffer
                | SirCallerArgumentRole::OutputBuffer
                | SirCallerArgumentRole::InputOutputBuffer,
                Some(_),
                SirDeclaredShapeV1::Ranked { dimensions },
            ) if !dimensions.is_empty() && dimensions.len() <= 16 => {}
            _ => {
                return Err(SirError::InvalidStructure(
                    "caller argument role/type/shape",
                ));
            }
        }
        Ok(())
    }

    /// Returns the ABI index.
    #[must_use]
    pub const fn index(&self) -> ArgumentIndex {
        self.index
    }
}

impl TryFrom<SirCallerArgumentWire> for SirCallerArgumentV1 {
    type Error = SirError;
    fn try_from(wire: SirCallerArgumentWire) -> Result<Self, Self::Error> {
        Self::new(
            wire.index,
            wire.name,
            wire.role,
            wire.data_type,
            wire.shape,
            wire.valid_domain,
        )
    }
}

impl<'de> Deserialize<'de> for SirCallerArgumentV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SirCallerArgumentWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Layer of desired semantics asserted by the caller or proposed by SIR.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SirIntentLayer {
    Algorithm,
    Numerical,
    ModelDeployment,
    ObservableContract,
}

/// One caller-authoritative desired-semantics claim.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SirCallerClaimV1 {
    id: SirCallerClaimId,
    layer: SirIntentLayer,
    statement: SirCallerClaimStatement,
    references: Vec<ContentId<SirCallerReferenceArtifact>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SirCallerClaimWire {
    id: SirCallerClaimId,
    layer: SirIntentLayer,
    statement: SirCallerClaimStatement,
    references: Vec<ContentId<SirCallerReferenceArtifact>>,
}

impl SirCallerClaimV1 {
    /// Creates one caller-authoritative intent claim.
    ///
    /// # Errors
    ///
    /// Rejects duplicate or non-canonical reference identities.
    pub fn new(
        id: SirCallerClaimId,
        layer: SirIntentLayer,
        statement: SirCallerClaimStatement,
        references: Vec<ContentId<SirCallerReferenceArtifact>>,
    ) -> Result<Self, SirError> {
        validate_sorted_content_ids(&references, "caller claim references")?;
        Ok(Self {
            id,
            layer,
            statement,
            references,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &SirCallerClaimId {
        &self.id
    }
}

impl TryFrom<SirCallerClaimWire> for SirCallerClaimV1 {
    type Error = SirError;
    fn try_from(wire: SirCallerClaimWire) -> Result<Self, Self::Error> {
        Self::new(wire.id, wire.layer, wire.statement, wire.references)
    }
}

impl<'de> Deserialize<'de> for SirCallerClaimV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SirCallerClaimWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// One caller-declared exclusion.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SirCallerExclusionV1 {
    id: SirCallerExclusionId,
    statement: SirExclusionStatement,
}

/// Kind of fact the caller explicitly leaves unknown.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SirDeclaredUnknownKind {
    ShapeOrDomain,
    ErrorBehavior,
    Algorithm,
    Numerical,
    ModelDeployment,
    ObservableContract,
}

/// One explicit caller unknown; absence is never treated as a declaration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SirDeclaredUnknownV1 {
    id: SirDeclaredUnknownId,
    kind: SirDeclaredUnknownKind,
    question: SirDeclaredUnknownQuestion,
}

/// Caller-owned minimum declaration before SIR inference.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "SirCallerDeclarationWire")]
pub struct SirCallerDeclarationV1 {
    schema_version: u16,
    source_entry_point: EntryPointName,
    arguments: Vec<SirCallerArgumentV1>,
    error_behaviors: Vec<SirErrorBehaviorDeclaration>,
    claims: Vec<SirCallerClaimV1>,
    exclusions: Vec<SirCallerExclusionV1>,
    unknowns: Vec<SirDeclaredUnknownV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SirCallerDeclarationWire {
    schema_version: u16,
    source_entry_point: EntryPointName,
    arguments: Vec<SirCallerArgumentV1>,
    error_behaviors: Vec<SirErrorBehaviorDeclaration>,
    claims: Vec<SirCallerClaimV1>,
    exclusions: Vec<SirCallerExclusionV1>,
    unknowns: Vec<SirDeclaredUnknownV1>,
}

impl TryFrom<SirCallerDeclarationWire> for SirCallerDeclarationV1 {
    type Error = SirError;

    fn try_from(wire: SirCallerDeclarationWire) -> Result<Self, Self::Error> {
        if wire.schema_version != SCHEMA_V1 {
            return Err(SirError::InvalidStructure("caller declaration schema"));
        }
        if wire.arguments.is_empty() || wire.arguments.len() > MAX_ARGUMENTS {
            return Err(SirError::InvalidStructure("caller argument count"));
        }
        for (expected, argument) in wire.arguments.iter().enumerate() {
            argument.validate()?;
            if usize::from(argument.index.get()) != expected {
                return Err(SirError::InvalidStructure(
                    "caller arguments must be in contiguous ABI order",
                ));
            }
        }
        if wire.claims.is_empty() || wire.claims.len() > MAX_CALLER_CLAIMS {
            return Err(SirError::InvalidStructure("caller claim count"));
        }
        validate_strict_ids(
            wire.claims.iter().map(|value| value.id.as_str()),
            "caller claim order",
        )?;
        for claim in &wire.claims {
            validate_sorted_content_ids(&claim.references, "caller claim references")?;
        }
        validate_strict_ids(
            wire.exclusions.iter().map(|value| value.id.as_str()),
            "caller exclusion order",
        )?;
        validate_strict_ids(
            wire.unknowns.iter().map(|value| value.id.as_str()),
            "caller unknown order",
        )?;
        if wire.error_behaviors.len() > MAX_DECLARATIONS
            || wire.exclusions.len() > MAX_DECLARATIONS
            || wire.unknowns.len() > MAX_DECLARATIONS
        {
            return Err(SirError::InvalidStructure(
                "caller declaration collection bound",
            ));
        }
        if wire.error_behaviors.is_empty()
            && !wire
                .unknowns
                .iter()
                .any(|value| value.kind == SirDeclaredUnknownKind::ErrorBehavior)
        {
            return Err(SirError::InvalidStructure(
                "caller must declare or explicitly leave error behavior unknown",
            ));
        }
        Ok(Self {
            schema_version: wire.schema_version,
            source_entry_point: wire.source_entry_point,
            arguments: wire.arguments,
            error_behaviors: wire.error_behaviors,
            claims: wire.claims,
            exclusions: wire.exclusions,
            unknowns: wire.unknowns,
        })
    }
}

impl SirCallerDeclarationV1 {
    #[must_use]
    pub fn claims(&self) -> &[SirCallerClaimV1] {
        &self.claims
    }

    #[must_use]
    pub fn unknowns(&self) -> &[SirDeclaredUnknownV1] {
        &self.unknowns
    }
}

impl SirDeclaredUnknownV1 {
    #[must_use]
    pub const fn id(&self) -> &SirDeclaredUnknownId {
        &self.id
    }

    #[must_use]
    pub const fn kind(&self) -> SirDeclaredUnknownKind {
        self.kind
    }

    #[must_use]
    pub const fn question(&self) -> &SirDeclaredUnknownQuestion {
        &self.question
    }
}

/// Exact target context selected by the Controller.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SirTargetContextV1 {
    soc: SirTargetSocSelectionV1,
    toolchain: SirTargetToolchainSelectionV1,
    environment: SirTargetEnvironmentSelectionV1,
}

impl SirTargetContextV1 {
    #[must_use]
    pub const fn new(
        soc: SirTargetSocSelectionV1,
        toolchain: SirTargetToolchainSelectionV1,
        environment: SirTargetEnvironmentSelectionV1,
    ) -> Self {
        Self {
            soc,
            toolchain,
            environment,
        }
    }
}

/// Target `SoC` is either explicitly selected or explicitly unresolved.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SirTargetSocSelectionV1 {
    Selected { soc: SirTargetSoc },
    NotSelected,
}

/// Target toolchain is either explicitly selected or explicitly unresolved.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SirTargetToolchainSelectionV1 {
    Selected { toolchain: SirTargetToolchain },
    NotSelected,
}

/// Target environment/ABI is either explicitly selected or explicitly unresolved.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SirTargetEnvironmentSelectionV1 {
    Selected { environment: TargetEnvironmentName },
    NotSelected,
}

/// Prior feedback is either explicitly absent or an exact allowlist.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SirPriorFeedbackV1 {
    NoPriorFeedback,
    Allowed {
        references: Vec<ContentId<SirPriorFeedbackArtifact>>,
    },
}

impl SirPriorFeedbackV1 {
    fn validate(&self) -> Result<(), SirError> {
        if let Self::Allowed { references } = self {
            if references.is_empty() || references.len() > MAX_AUTHORIZED_EVIDENCE {
                return Err(SirError::InvalidStructure("prior feedback count"));
            }
            validate_sorted_content_ids(references, "prior feedback order")?;
        }
        Ok(())
    }
}

/// Caller/request material before trusted task and capability binding.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "IntentRecoveryRequestWire")]
pub struct IntentRecoveryRequestV1 {
    schema_version: u16,
    caller: SirCallerDeclarationV1,
    target: SirTargetContextV1,
    authorized_evidence: Vec<ContentId<SirAuthorizedEvidenceArtifact>>,
    prior_feedback: SirPriorFeedbackV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentRecoveryRequestWire {
    schema_version: u16,
    caller: SirCallerDeclarationV1,
    target: SirTargetContextV1,
    authorized_evidence: Vec<ContentId<SirAuthorizedEvidenceArtifact>>,
    prior_feedback: SirPriorFeedbackV1,
}

impl TryFrom<IntentRecoveryRequestWire> for IntentRecoveryRequestV1 {
    type Error = SirError;

    fn try_from(wire: IntentRecoveryRequestWire) -> Result<Self, Self::Error> {
        if wire.schema_version != SCHEMA_V1 {
            return Err(SirError::InvalidStructure("recovery request schema"));
        }
        if wire.authorized_evidence.len() > MAX_AUTHORIZED_EVIDENCE {
            return Err(SirError::InvalidStructure("authorized evidence count"));
        }
        validate_sorted_content_ids(&wire.authorized_evidence, "authorized evidence order")?;
        wire.prior_feedback.validate()?;
        Ok(Self {
            schema_version: wire.schema_version,
            caller: wire.caller,
            target: wire.target,
            authorized_evidence: wire.authorized_evidence,
            prior_feedback: wire.prior_feedback,
        })
    }
}

impl IntentRecoveryRequestV1 {
    #[must_use]
    pub const fn caller(&self) -> &SirCallerDeclarationV1 {
        &self.caller
    }
}

/// Exact capability granted to the SIR process/harness.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SirCapability {
    ReadTaskArtifact,
    SubmitIntentHypothesisSet,
}

/// Frozen proposal-only capability manifest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SirCapabilityManifestV1 {
    schema_version: u16,
    capabilities: Vec<SirCapability>,
    task_limits: SirTaskLimits,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SirCapabilityManifestWire {
    schema_version: u16,
    capabilities: Vec<SirCapability>,
    task_limits: SirTaskLimits,
}

impl SirCapabilityManifestV1 {
    #[must_use]
    pub fn proposal_only(task_limits: SirTaskLimits) -> Self {
        Self {
            schema_version: SCHEMA_V1,
            capabilities: vec![
                SirCapability::ReadTaskArtifact,
                SirCapability::SubmitIntentHypothesisSet,
            ],
            task_limits,
        }
    }

    fn validate(&self) -> Result<(), SirError> {
        if self.schema_version != SCHEMA_V1
            || self.capabilities
                != [
                    SirCapability::ReadTaskArtifact,
                    SirCapability::SubmitIntentHypothesisSet,
                ]
        {
            return Err(SirError::InvalidStructure("SIR capability manifest"));
        }
        Ok(())
    }
}

impl TryFrom<SirCapabilityManifestWire> for SirCapabilityManifestV1 {
    type Error = SirError;
    fn try_from(wire: SirCapabilityManifestWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            capabilities: wire.capabilities,
            task_limits: wire.task_limits,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for SirCapabilityManifestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SirCapabilityManifestWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Frozen task, caller, target, feedback and capability input for one recovery run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentRecoveryInputV1 {
    schema_version: u16,
    task_id: TaskId,
    task_bundle: ContentId<SirTaskBundleArtifact>,
    request: IntentRecoveryRequestV1,
    capability_manifest: SirCapabilityManifestV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentRecoveryInputWire {
    schema_version: u16,
    task_id: TaskId,
    task_bundle: ContentId<SirTaskBundleArtifact>,
    request: IntentRecoveryRequestV1,
    capability_manifest: SirCapabilityManifestV1,
}

impl IntentRecoveryInputV1 {
    /// Binds a caller request to the exact task and trusted capability manifest.
    ///
    /// # Errors
    ///
    /// Rejects a capability manifest outside the proposal-only current-V1 profile.
    pub fn new(
        task_id: TaskId,
        task_bundle: ContentId<SirTaskBundleArtifact>,
        request: IntentRecoveryRequestV1,
        capability_manifest: SirCapabilityManifestV1,
    ) -> Result<Self, SirError> {
        capability_manifest.validate()?;
        Ok(Self {
            schema_version: SCHEMA_V1,
            task_id,
            task_bundle,
            request,
            capability_manifest,
        })
    }

    #[must_use]
    pub const fn task_bundle(&self) -> ContentId<SirTaskBundleArtifact> {
        self.task_bundle
    }

    #[must_use]
    pub const fn request(&self) -> &IntentRecoveryRequestV1 {
        &self.request
    }

    /// Derives the exact frozen input identity.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or identity derivation fails.
    pub fn identity(&self) -> Result<ContentId<IntentRecoveryInputArtifact>, SirError> {
        let bytes =
            cairn_codec::to_vec(self).map_err(|error| SirError::Codec(error.to_string()))?;
        ContentId::derive(&bytes).map_err(|error| SirError::Codec(error.to_string()))
    }
}

impl TryFrom<IntentRecoveryInputWire> for IntentRecoveryInputV1 {
    type Error = SirError;

    fn try_from(wire: IntentRecoveryInputWire) -> Result<Self, Self::Error> {
        if wire.schema_version != SCHEMA_V1 {
            return Err(SirError::InvalidStructure("recovery input schema"));
        }
        Self::new(
            wire.task_id,
            wire.task_bundle,
            wire.request,
            wire.capability_manifest,
        )
    }
}

impl<'de> Deserialize<'de> for IntentRecoveryInputV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentRecoveryInputWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// A source-observed fact, distinct from caller declaration and intent hypothesis.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SirObservedFactV1 {
    id: SirObservationId,
    statement: SirObservationStatement,
    citations: Vec<SirSourceCitationV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SirObservedFactWire {
    id: SirObservationId,
    statement: SirObservationStatement,
    citations: Vec<SirSourceCitationV1>,
}

impl SirObservedFactV1 {
    fn validate(&self) -> Result<(), SirError> {
        if self.citations.is_empty() || self.citations.len() > MAX_CITATIONS {
            return Err(SirError::InvalidStructure("observed fact citation count"));
        }
        Ok(())
    }
}

impl TryFrom<SirObservedFactWire> for SirObservedFactV1 {
    type Error = SirError;
    fn try_from(wire: SirObservedFactWire) -> Result<Self, Self::Error> {
        let value = Self {
            id: wire.id,
            statement: wire.statement,
            citations: wire.citations,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for SirObservedFactV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SirObservedFactWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Evidence edge that preserves whether a statement came from caller authority or source observation.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(tag = "source", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SirIntentEvidenceRefV1 {
    CallerClaim { claim: SirCallerClaimId },
    ObservedFact { observation: SirObservationId },
}

/// One proposed higher-order intent claim.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SirIntentHypothesisV1 {
    id: SirHypothesisId,
    layer: SirIntentLayer,
    claim: SirHypothesisClaim,
    domain: SirIntentDomain,
    supporting_evidence: Vec<SirIntentEvidenceRefV1>,
    counter_evidence: Vec<SirIntentEvidenceRefV1>,
}

impl SirIntentHypothesisV1 {
    #[must_use]
    pub const fn id(&self) -> &SirHypothesisId {
        &self.id
    }

    #[must_use]
    pub const fn layer(&self) -> SirIntentLayer {
        self.layer
    }

    #[must_use]
    pub const fn claim(&self) -> &SirHypothesisClaim {
        &self.claim
    }

    #[must_use]
    pub const fn domain(&self) -> &SirIntentDomain {
        &self.domain
    }
}

/// Claim edge used by a conflict without erasing caller/hypothesis identity.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(tag = "source", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SirIntentClaimRefV1 {
    CallerClaim { claim: SirCallerClaimId },
    Hypothesis { hypothesis: SirHypothesisId },
}

/// Explicit conflict between claims that must not be flattened.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SirIntentConflictV1 {
    id: SirConflictId,
    statement: SirConflictStatement,
    claims: Vec<SirIntentClaimRefV1>,
    evidence: Vec<SirIntentEvidenceRefV1>,
}

impl SirIntentConflictV1 {
    #[must_use]
    pub const fn id(&self) -> &SirConflictId {
        &self.id
    }

    #[must_use]
    pub fn claims(&self) -> &[SirIntentClaimRefV1] {
        &self.claims
    }
}

/// Classification of an unresolved question.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SirUnknownKind {
    DesiredSemantics,
    SourceBehavior,
    NumericalAllowance,
    DeploymentContext,
    ToolOrEvidenceGap,
}

/// Explicit unresolved question.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SirUnknownV1 {
    id: SirUnknownId,
    kind: SirUnknownKind,
    question: SirUnknownQuestion,
    evidence: Vec<SirIntentEvidenceRefV1>,
}

impl SirUnknownV1 {
    #[must_use]
    pub const fn id(&self) -> &SirUnknownId {
        &self.id
    }

    #[must_use]
    pub const fn kind(&self) -> SirUnknownKind {
        self.kind
    }

    #[must_use]
    pub const fn question(&self) -> &SirUnknownQuestion {
        &self.question
    }
}

/// Proposed invariant that a later Admission may promote.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SirSemanticInvariantV1 {
    id: SirInvariantId,
    statement: SirInvariantStatement,
    evidence: Vec<SirIntentEvidenceRefV1>,
}

/// Proposed optimization freedom tied to exact protected invariants.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SirOptimizationFreedomV1 {
    id: SirOptimizationFreedomId,
    statement: SirOptimizationFreedomStatement,
    protected_invariants: Vec<SirInvariantId>,
    evidence: Vec<SirIntentEvidenceRefV1>,
}

/// Proposed disposition for one observed source behavior.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SirSourceBehaviorDispositionKind {
    PreserveObservedBehavior,
    FollowProposedSemanticIntent,
    ExcludeUndefinedRegion,
    SplitDomain,
    BlockPendingUserDecision,
    UnknownClassification,
}

/// Non-authoritative source-behavior disposition.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SirSourceBehaviorDispositionV1 {
    id: SirSourceDispositionId,
    observation: SirObservationId,
    disposition: SirSourceBehaviorDispositionKind,
    rationale: SirDispositionRationale,
    evidence: Vec<SirIntentEvidenceRefV1>,
}

/// Exact unresolved target for a proposed discriminating experiment.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SirDisambiguationTargetV1 {
    Hypothesis { hypothesis: SirHypothesisId },
    Conflict { conflict: SirConflictId },
    Unknown { unknown: SirUnknownId },
}

/// Experiment proposal; SIR cannot claim it ran.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SirDisambiguationExperimentV1 {
    id: SirExperimentId,
    targets: Vec<SirDisambiguationTargetV1>,
    plan: SirExperimentPlan,
    predictions: Vec<SirExperimentPrediction>,
}

impl SirDisambiguationExperimentV1 {
    #[must_use]
    pub fn targets(&self) -> &[SirDisambiguationTargetV1] {
        &self.targets
    }
}

/// Complete model-authored proposal body.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SirProposalSubmissionV1 {
    schema_version: u16,
    observed_facts: Vec<SirObservedFactV1>,
    hypotheses: Vec<SirIntentHypothesisV1>,
    conflicts: Vec<SirIntentConflictV1>,
    unknowns: Vec<SirUnknownV1>,
    invariants: Vec<SirSemanticInvariantV1>,
    optimization_freedoms: Vec<SirOptimizationFreedomV1>,
    source_dispositions: Vec<SirSourceBehaviorDispositionV1>,
    disambiguation_experiments: Vec<SirDisambiguationExperimentV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SirProposalSubmissionWire {
    schema_version: u16,
    observed_facts: Vec<SirObservedFactV1>,
    hypotheses: Vec<SirIntentHypothesisV1>,
    conflicts: Vec<SirIntentConflictV1>,
    unknowns: Vec<SirUnknownV1>,
    invariants: Vec<SirSemanticInvariantV1>,
    optimization_freedoms: Vec<SirOptimizationFreedomV1>,
    source_dispositions: Vec<SirSourceBehaviorDispositionV1>,
    disambiguation_experiments: Vec<SirDisambiguationExperimentV1>,
}

impl TryFrom<SirProposalSubmissionWire> for SirProposalSubmissionV1 {
    type Error = SirError;

    fn try_from(wire: SirProposalSubmissionWire) -> Result<Self, Self::Error> {
        if wire.schema_version != SCHEMA_V1 {
            return Err(SirError::InvalidStructure("proposal schema"));
        }
        validate_count(
            &wire.observed_facts,
            1,
            MAX_OBSERVATIONS,
            "observed fact count",
        )?;
        validate_count(&wire.hypotheses, 2, MAX_HYPOTHESES, "hypothesis count")?;
        validate_count(&wire.conflicts, 1, MAX_CONFLICTS, "conflict count")?;
        validate_count(&wire.unknowns, 1, MAX_UNKNOWNS, "unknown count")?;
        validate_count(&wire.invariants, 1, MAX_INVARIANTS, "invariant count")?;
        validate_count(
            &wire.optimization_freedoms,
            0,
            MAX_FREEDOMS,
            "optimization freedom count",
        )?;
        validate_count(
            &wire.source_dispositions,
            0,
            MAX_DISPOSITIONS,
            "source disposition count",
        )?;
        validate_count(
            &wire.disambiguation_experiments,
            0,
            MAX_EXPERIMENTS,
            "experiment count",
        )?;
        validate_strict_ids(
            wire.observed_facts.iter().map(|value| value.id.as_str()),
            "observation order",
        )?;
        validate_strict_ids(
            wire.hypotheses.iter().map(|value| value.id.as_str()),
            "hypothesis order",
        )?;
        validate_strict_ids(
            wire.conflicts.iter().map(|value| value.id.as_str()),
            "conflict order",
        )?;
        validate_strict_ids(
            wire.unknowns.iter().map(|value| value.id.as_str()),
            "unknown order",
        )?;
        validate_strict_ids(
            wire.invariants.iter().map(|value| value.id.as_str()),
            "invariant order",
        )?;
        validate_strict_ids(
            wire.optimization_freedoms
                .iter()
                .map(|value| value.id.as_str()),
            "freedom order",
        )?;
        validate_strict_ids(
            wire.source_dispositions
                .iter()
                .map(|value| value.id.as_str()),
            "disposition order",
        )?;
        validate_strict_ids(
            wire.disambiguation_experiments
                .iter()
                .map(|value| value.id.as_str()),
            "experiment order",
        )?;
        let value = Self {
            schema_version: wire.schema_version,
            observed_facts: wire.observed_facts,
            hypotheses: wire.hypotheses,
            conflicts: wire.conflicts,
            unknowns: wire.unknowns,
            invariants: wire.invariants,
            optimization_freedoms: wire.optimization_freedoms,
            source_dispositions: wire.source_dispositions,
            disambiguation_experiments: wire.disambiguation_experiments,
        };
        value.validate_internal()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for SirProposalSubmissionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SirProposalSubmissionWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl SirProposalSubmissionV1 {
    #[must_use]
    pub fn observed_facts(&self) -> &[SirObservedFactV1] {
        &self.observed_facts
    }
    #[must_use]
    pub fn hypotheses(&self) -> &[SirIntentHypothesisV1] {
        &self.hypotheses
    }
    #[must_use]
    pub fn conflicts(&self) -> &[SirIntentConflictV1] {
        &self.conflicts
    }
    #[must_use]
    pub fn unknowns(&self) -> &[SirUnknownV1] {
        &self.unknowns
    }
    #[must_use]
    pub fn invariants(&self) -> &[SirSemanticInvariantV1] {
        &self.invariants
    }

    #[must_use]
    pub fn disambiguation_experiments(&self) -> &[SirDisambiguationExperimentV1] {
        &self.disambiguation_experiments
    }

    fn validate_internal(&self) -> Result<(), SirError> {
        let observations = self
            .observed_facts
            .iter()
            .map(|value| value.id.as_str())
            .collect::<HashSet<_>>();
        let hypotheses = self
            .hypotheses
            .iter()
            .map(|value| value.id.as_str())
            .collect::<HashSet<_>>();
        let conflicts = self
            .conflicts
            .iter()
            .map(|value| value.id.as_str())
            .collect::<HashSet<_>>();
        let unknowns = self
            .unknowns
            .iter()
            .map(|value| value.id.as_str())
            .collect::<HashSet<_>>();
        let invariants = self
            .invariants
            .iter()
            .map(|value| value.id.as_str())
            .collect::<HashSet<_>>();
        for fact in &self.observed_facts {
            fact.validate()?;
        }
        for hypothesis in &self.hypotheses {
            validate_edges(
                &hypothesis.supporting_evidence,
                1,
                "hypothesis supporting evidence",
            )?;
            validate_edges(
                &hypothesis.counter_evidence,
                0,
                "hypothesis counter evidence",
            )?;
            validate_internal_evidence_refs(&hypothesis.supporting_evidence, &observations)?;
            validate_internal_evidence_refs(&hypothesis.counter_evidence, &observations)?;
        }
        for conflict in &self.conflicts {
            validate_edges(&conflict.claims, 2, "conflict claims")?;
            validate_edges(&conflict.evidence, 0, "conflict evidence")?;
            if !conflict
                .claims
                .iter()
                .any(|claim| matches!(claim, SirIntentClaimRefV1::Hypothesis { .. }))
            {
                return Err(SirError::InvalidStructure(
                    "conflict must include a hypothesis",
                ));
            }
            for claim in &conflict.claims {
                if let SirIntentClaimRefV1::Hypothesis { hypothesis } = claim {
                    if !hypotheses.contains(hypothesis.as_str()) {
                        return Err(SirError::InvalidStructure("dangling conflict hypothesis"));
                    }
                }
            }
            validate_internal_evidence_refs(&conflict.evidence, &observations)?;
        }
        for unknown in &self.unknowns {
            validate_edges(&unknown.evidence, 0, "unknown evidence")?;
            validate_internal_evidence_refs(&unknown.evidence, &observations)?;
        }
        for invariant in &self.invariants {
            validate_edges(&invariant.evidence, 1, "invariant evidence")?;
            validate_internal_evidence_refs(&invariant.evidence, &observations)?;
        }
        for freedom in &self.optimization_freedoms {
            validate_edges(&freedom.protected_invariants, 1, "protected invariants")?;
            validate_edges(&freedom.evidence, 1, "optimization freedom evidence")?;
            if freedom
                .protected_invariants
                .iter()
                .any(|id| !invariants.contains(id.as_str()))
            {
                return Err(SirError::InvalidStructure("dangling protected invariant"));
            }
            validate_internal_evidence_refs(&freedom.evidence, &observations)?;
        }
        for disposition in &self.source_dispositions {
            if !observations.contains(disposition.observation.as_str()) {
                return Err(SirError::InvalidStructure(
                    "dangling disposition observation",
                ));
            }
            validate_edges(&disposition.evidence, 1, "source disposition evidence")?;
            validate_internal_evidence_refs(&disposition.evidence, &observations)?;
        }
        validate_experiment_targets(
            &self.disambiguation_experiments,
            &hypotheses,
            &conflicts,
            &unknowns,
        )?;
        Ok(())
    }

    #[cfg(feature = "agent-runtime")]
    pub(crate) fn validate_against(
        &self,
        workspace: &SirTaskWorkspace,
        input: &IntentRecoveryInputV1,
    ) -> Result<(), SirError> {
        self.validate_internal()?;
        for fact in &self.observed_facts {
            for citation in &fact.citations {
                workspace.validate_citation(citation)?;
            }
        }
        self.validate_against_recovery_input(input)
    }

    pub(crate) fn validate_against_recovery_input(
        &self,
        input: &IntentRecoveryInputV1,
    ) -> Result<(), SirError> {
        let caller_claims = input
            .request
            .caller
            .claims
            .iter()
            .map(|value| value.id.as_str())
            .collect::<HashSet<_>>();
        for hypothesis in &self.hypotheses {
            validate_caller_evidence_refs(&hypothesis.supporting_evidence, &caller_claims)?;
            validate_caller_evidence_refs(&hypothesis.counter_evidence, &caller_claims)?;
        }
        for conflict in &self.conflicts {
            for claim in &conflict.claims {
                if let SirIntentClaimRefV1::CallerClaim { claim } = claim {
                    if !caller_claims.contains(claim.as_str()) {
                        return Err(SirError::InvalidStructure("dangling caller claim"));
                    }
                }
            }
            validate_caller_evidence_refs(&conflict.evidence, &caller_claims)?;
        }
        for unknown in &self.unknowns {
            validate_caller_evidence_refs(&unknown.evidence, &caller_claims)?;
        }
        for invariant in &self.invariants {
            validate_caller_evidence_refs(&invariant.evidence, &caller_claims)?;
        }
        for freedom in &self.optimization_freedoms {
            validate_caller_evidence_refs(&freedom.evidence, &caller_claims)?;
        }
        for disposition in &self.source_dispositions {
            validate_caller_evidence_refs(&disposition.evidence, &caller_claims)?;
        }
        Ok(())
    }
}

/// Trusted provenance envelope around one complete SIR proposal.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentHypothesisSetProposalV1 {
    schema_version: u16,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    episode_id: EpisodeId,
    model_configuration: ContentId<SirResolvedRuntimeModelArtifact>,
    submission: SirProposalSubmissionV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentHypothesisSetProposalWire {
    schema_version: u16,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    episode_id: EpisodeId,
    model_configuration: ContentId<SirResolvedRuntimeModelArtifact>,
    submission: SirProposalSubmissionV1,
}

impl IntentHypothesisSetProposalV1 {
    #[cfg(feature = "agent-runtime")]
    pub(crate) fn new(
        recovery_input: ContentId<IntentRecoveryInputArtifact>,
        episode_id: EpisodeId,
        model_configuration: ContentId<SirResolvedRuntimeModelArtifact>,
        submission: SirProposalSubmissionV1,
    ) -> Self {
        Self {
            schema_version: SCHEMA_V1,
            recovery_input,
            episode_id,
            model_configuration,
            submission,
        }
    }

    #[must_use]
    pub const fn recovery_input(&self) -> ContentId<IntentRecoveryInputArtifact> {
        self.recovery_input
    }
    #[must_use]
    pub const fn submission(&self) -> &SirProposalSubmissionV1 {
        &self.submission
    }

    #[must_use]
    pub const fn episode_id(&self) -> EpisodeId {
        self.episode_id
    }

    /// Derives the exact proposal identity.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or identity derivation fails.
    pub fn identity(&self) -> Result<ContentId<SirIntentHypothesisSetProposalArtifact>, SirError> {
        let bytes =
            cairn_codec::to_vec(self).map_err(|error| SirError::Codec(error.to_string()))?;
        ContentId::derive(&bytes).map_err(|error| SirError::Codec(error.to_string()))
    }
}

impl TryFrom<IntentHypothesisSetProposalWire> for IntentHypothesisSetProposalV1 {
    type Error = SirError;
    fn try_from(wire: IntentHypothesisSetProposalWire) -> Result<Self, Self::Error> {
        if wire.schema_version != SCHEMA_V1 {
            return Err(SirError::InvalidStructure("proposal envelope schema"));
        }
        Ok(Self {
            schema_version: wire.schema_version,
            recovery_input: wire.recovery_input,
            episode_id: wire.episode_id,
            model_configuration: wire.model_configuration,
            submission: wire.submission,
        })
    }
}

impl<'de> Deserialize<'de> for IntentHypothesisSetProposalV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentHypothesisSetProposalWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

fn validate_count<T>(
    values: &[T],
    minimum: usize,
    maximum: usize,
    field: &'static str,
) -> Result<(), SirError> {
    if !(minimum..=maximum).contains(&values.len()) {
        return Err(SirError::InvalidStructure(field));
    }
    Ok(())
}

fn validate_edges<T: Eq + std::hash::Hash>(
    values: &[T],
    minimum: usize,
    field: &'static str,
) -> Result<(), SirError> {
    if values.len() < minimum
        || values.len() > MAX_EDGES
        || values.iter().collect::<HashSet<_>>().len() != values.len()
    {
        return Err(SirError::InvalidStructure(field));
    }
    Ok(())
}

fn validate_strict_ids<'a>(
    values: impl Iterator<Item = &'a str>,
    field: &'static str,
) -> Result<(), SirError> {
    let mut previous = None;
    for value in values {
        if previous.is_some_and(|prior| prior >= value) {
            return Err(SirError::InvalidStructure(field));
        }
        previous = Some(value);
    }
    Ok(())
}

fn validate_sorted_content_ids<T: ContentType>(
    values: &[ContentId<T>],
    field: &'static str,
) -> Result<(), SirError> {
    let wires = values.iter().map(ContentId::to_wire).collect::<Vec<_>>();
    if wires.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(SirError::InvalidStructure(field));
    }
    Ok(())
}

fn validate_caller_evidence_refs(
    values: &[SirIntentEvidenceRefV1],
    caller_claims: &HashSet<&str>,
) -> Result<(), SirError> {
    if values.iter().any(|value| {
        matches!(value, SirIntentEvidenceRefV1::CallerClaim { claim } if !caller_claims.contains(claim.as_str()))
    }) {
        return Err(SirError::InvalidStructure("dangling caller claim"));
    }
    Ok(())
}

fn validate_experiment_targets(
    experiments: &[SirDisambiguationExperimentV1],
    hypotheses: &HashSet<&str>,
    conflicts: &HashSet<&str>,
    unknowns: &HashSet<&str>,
) -> Result<(), SirError> {
    for experiment in experiments {
        validate_edges(&experiment.targets, 1, "experiment targets")?;
        validate_edges(&experiment.predictions, 2, "experiment predictions")?;
        for target in &experiment.targets {
            let valid = match target {
                SirDisambiguationTargetV1::Hypothesis { hypothesis } => {
                    hypotheses.contains(hypothesis.as_str())
                }
                SirDisambiguationTargetV1::Conflict { conflict } => {
                    conflicts.contains(conflict.as_str())
                }
                SirDisambiguationTargetV1::Unknown { unknown } => {
                    unknowns.contains(unknown.as_str())
                }
            };
            if !valid {
                return Err(SirError::InvalidStructure("dangling experiment target"));
            }
        }
    }
    Ok(())
}

fn validate_internal_evidence_refs(
    values: &[SirIntentEvidenceRefV1],
    observations: &HashSet<&str>,
) -> Result<(), SirError> {
    if values.iter().any(|value| {
        matches!(value, SirIntentEvidenceRefV1::ObservedFact { observation } if !observations.contains(observation.as_str()))
    }) {
        return Err(SirError::InvalidStructure("dangling observed-fact reference"));
    }
    Ok(())
}
