//! Task-generic operation semantics promoted by independent Intent Admission.

use cairn_protocol::ContentType;
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    SirCallerClaimV1, SirHypothesisClaim, SirIntentDomain, SirIntentHypothesisV1, SirIntentLayer,
};

const MAX_AUTHORITY_CLAIMS: usize = 16;

/// First immutable admitted migration-intent contract identity.
pub enum MigrationIntentContractArtifact {}

impl ContentType for MigrationIntentContractArtifact {
    const DOMAIN: &'static str = "migration.intent-contract.v1";
}

/// Exact operation semantics stated or promoted by the authenticated task authority.
///
/// This contract deliberately does not prescribe a collection, reduction, tensor, kernel, or
/// fixture shape. Later Oracle exploration interprets the protected semantic statement and domain
/// for the submitted task.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OperationIntentV1 {
    caller_claims: Vec<SirCallerClaimV1>,
    layer: SirIntentLayer,
    semantics: SirHypothesisClaim,
    domain: SirIntentDomain,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationIntentWire {
    caller_claims: Vec<SirCallerClaimV1>,
    layer: SirIntentLayer,
    semantics: SirHypothesisClaim,
    domain: SirIntentDomain,
}

impl OperationIntentV1 {
    /// Creates one generic, claim-scoped operation contract.
    ///
    /// # Errors
    ///
    /// Rejects an empty, duplicate, unsorted, or over-broad authority claim set.
    pub fn new(
        caller_claims: Vec<SirCallerClaimV1>,
        layer: SirIntentLayer,
        semantics: SirHypothesisClaim,
        domain: SirIntentDomain,
    ) -> Result<Self, crate::SirError> {
        if caller_claims.is_empty()
            || caller_claims.len() > MAX_AUTHORITY_CLAIMS
            || caller_claims
                .windows(2)
                .any(|pair| pair[0].id() >= pair[1].id())
        {
            return Err(crate::SirError::InvalidStructure(
                "operation intent authority claim order",
            ));
        }
        Ok(Self {
            caller_claims,
            layer,
            semantics,
            domain,
        })
    }

    #[must_use]
    pub fn caller_claims(&self) -> &[SirCallerClaimV1] {
        &self.caller_claims
    }

    #[must_use]
    pub const fn layer(&self) -> SirIntentLayer {
        self.layer
    }

    #[must_use]
    pub const fn semantics(&self) -> &SirHypothesisClaim {
        &self.semantics
    }

    #[must_use]
    pub const fn domain(&self) -> &SirIntentDomain {
        &self.domain
    }

    #[must_use]
    pub fn matches_hypothesis(&self, hypothesis: &SirIntentHypothesisV1) -> bool {
        self.layer == hypothesis.layer()
            && &self.semantics == hypothesis.claim()
            && &self.domain == hypothesis.domain()
    }
}

impl TryFrom<OperationIntentWire> for OperationIntentV1 {
    type Error = crate::SirError;

    fn try_from(wire: OperationIntentWire) -> Result<Self, Self::Error> {
        Self::new(wire.caller_claims, wire.layer, wire.semantics, wire.domain)
    }
}

impl<'de> Deserialize<'de> for OperationIntentV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        OperationIntentWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Current task-generic claim admitted into the migration contract.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoritativeIntentClaimV1 {
    operation: OperationIntentV1,
}

impl AuthoritativeIntentClaimV1 {
    #[must_use]
    pub fn new(operation: OperationIntentV1) -> Self {
        Self { operation }
    }

    #[must_use]
    pub const fn operation(&self) -> &OperationIntentV1 {
        &self.operation
    }
}
