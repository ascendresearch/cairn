//! Candidate search inputs projected from published public authority.

use cairn_protocol::{ContentId, ContentType, SchemaVersion, TaskId};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    AdmittedCollectionOracleClaimArtifact, CollectionOracleClaimDomainV1,
    CollectionOracleClaimStrengthV1, IntentRecoveryInputArtifact, MigrationIntentContractArtifact,
    SirCallerClaimId,
};

/// Public Oracle Admission outcome consumed by Candidate search.
pub enum CollectionOracleAdmissionPublicOutcomeArtifact {}

impl ContentType for CollectionOracleAdmissionPublicOutcomeArtifact {
    const DOMAIN: &'static str = "migration.oracle-collection-admission-outcome-public.v1";
}

/// Model-visible input for one local-Oracle Candidate search episode.
pub enum CollectionCandidateSearchInputArtifact {}

impl ContentType for CollectionCandidateSearchInputArtifact {
    const DOMAIN: &'static str = "migration.candidate-collection-search-input.v1";
}

/// Explicit boundary preventing a local claim from granting verdict or release authority.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionCandidateSearchScopeV1 {
    LocalOracleExplorationOnly,
}

/// Strong fields supplied only after an Admission public outcome has been committed.
pub struct CollectionCandidateSearchAuthorityInput {
    task_id: TaskId,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    intent_contract: ContentId<MigrationIntentContractArtifact>,
    oracle_outcome: ContentId<CollectionOracleAdmissionPublicOutcomeArtifact>,
    oracle_claim: ContentId<AdmittedCollectionOracleClaimArtifact>,
    selection_claim: SirCallerClaimId,
    domain: CollectionOracleClaimDomainV1,
    strength: CollectionOracleClaimStrengthV1,
}

impl CollectionCandidateSearchAuthorityInput {
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "each authority edge is a distinct strong type and must remain explicit"
    )]
    pub const fn new(
        task_id: TaskId,
        recovery_input: ContentId<IntentRecoveryInputArtifact>,
        intent_contract: ContentId<MigrationIntentContractArtifact>,
        oracle_outcome: ContentId<CollectionOracleAdmissionPublicOutcomeArtifact>,
        oracle_claim: ContentId<AdmittedCollectionOracleClaimArtifact>,
        selection_claim: SirCallerClaimId,
        domain: CollectionOracleClaimDomainV1,
        strength: CollectionOracleClaimStrengthV1,
    ) -> Self {
        Self {
            task_id,
            recovery_input,
            intent_contract,
            oracle_outcome,
            oracle_claim,
            selection_claim,
            domain,
            strength,
        }
    }
}

/// Public, answer-free authority projection for the first Candidate search consumer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CollectionCandidateSearchInputV1 {
    schema_version: SchemaVersion,
    task_id: TaskId,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    intent_contract: ContentId<MigrationIntentContractArtifact>,
    oracle_outcome: ContentId<CollectionOracleAdmissionPublicOutcomeArtifact>,
    oracle_claim: ContentId<AdmittedCollectionOracleClaimArtifact>,
    selection_claim: SirCallerClaimId,
    domain: CollectionOracleClaimDomainV1,
    strength: CollectionOracleClaimStrengthV1,
    scope: CollectionCandidateSearchScopeV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionCandidateSearchInputWire {
    schema_version: SchemaVersion,
    task_id: TaskId,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    intent_contract: ContentId<MigrationIntentContractArtifact>,
    oracle_outcome: ContentId<CollectionOracleAdmissionPublicOutcomeArtifact>,
    oracle_claim: ContentId<AdmittedCollectionOracleClaimArtifact>,
    selection_claim: SirCallerClaimId,
    domain: CollectionOracleClaimDomainV1,
    strength: CollectionOracleClaimStrengthV1,
    scope: CollectionCandidateSearchScopeV1,
}

impl CollectionCandidateSearchInputV1 {
    fn validate_structure(&self) -> Result<(), CandidateSearchInputError> {
        if self.schema_version != schema_v1()
            || self.scope != CollectionCandidateSearchScopeV1::LocalOracleExplorationOnly
            || self.domain != CollectionOracleClaimDomainV1::FiniteNormalF32StrictlyAboveThreshold
            || self.strength
                != CollectionOracleClaimStrengthV1::ExactOccurrenceMultisetAndReportedCount
        {
            return Err(CandidateSearchInputError::InvalidStructure);
        }
        Ok(())
    }

    /// Revalidates every field against the exact authority projection.
    ///
    /// # Errors
    ///
    /// Rejects any duplicated authority edge that no longer agrees.
    pub fn validate_authority(
        &self,
        authority: &CollectionCandidateSearchAuthorityInput,
    ) -> Result<(), CandidateSearchInputError> {
        self.validate_structure()?;
        if self.task_id != authority.task_id
            || self.recovery_input != authority.recovery_input
            || self.intent_contract != authority.intent_contract
            || self.oracle_outcome != authority.oracle_outcome
            || self.oracle_claim != authority.oracle_claim
            || self.selection_claim != authority.selection_claim
            || self.domain != authority.domain
            || self.strength != authority.strength
        {
            return Err(CandidateSearchInputError::BindingMismatch);
        }
        Ok(())
    }

    /// Derives the exact Candidate input identity.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or identity derivation fails.
    pub fn identity(
        &self,
    ) -> Result<ContentId<CollectionCandidateSearchInputArtifact>, CandidateSearchInputError> {
        self.validate_structure()?;
        let bytes = cairn_codec::to_vec(self).map_err(codec)?;
        ContentId::derive(&bytes).map_err(codec)
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn recovery_input(&self) -> ContentId<IntentRecoveryInputArtifact> {
        self.recovery_input
    }

    #[must_use]
    pub const fn intent_contract(&self) -> ContentId<MigrationIntentContractArtifact> {
        self.intent_contract
    }

    #[must_use]
    pub const fn oracle_outcome(
        &self,
    ) -> ContentId<CollectionOracleAdmissionPublicOutcomeArtifact> {
        self.oracle_outcome
    }

    #[must_use]
    pub const fn oracle_claim(&self) -> ContentId<AdmittedCollectionOracleClaimArtifact> {
        self.oracle_claim
    }

    #[must_use]
    pub const fn selection_claim(&self) -> &SirCallerClaimId {
        &self.selection_claim
    }

    #[must_use]
    pub const fn domain(&self) -> CollectionOracleClaimDomainV1 {
        self.domain
    }

    #[must_use]
    pub const fn strength(&self) -> CollectionOracleClaimStrengthV1 {
        self.strength
    }

    #[must_use]
    pub const fn scope(&self) -> CollectionCandidateSearchScopeV1 {
        self.scope
    }
}

impl TryFrom<CollectionCandidateSearchInputWire> for CollectionCandidateSearchInputV1 {
    type Error = CandidateSearchInputError;

    fn try_from(wire: CollectionCandidateSearchInputWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            task_id: wire.task_id,
            recovery_input: wire.recovery_input,
            intent_contract: wire.intent_contract,
            oracle_outcome: wire.oracle_outcome,
            oracle_claim: wire.oracle_claim,
            selection_claim: wire.selection_claim,
            domain: wire.domain,
            strength: wire.strength,
            scope: wire.scope,
        };
        value.validate_structure()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CollectionCandidateSearchInputV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CollectionCandidateSearchInputWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Canonical Candidate search input ready for Controller archival and episode launch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCollectionCandidateSearchInput {
    input: CollectionCandidateSearchInputV1,
    bytes: Vec<u8>,
    id: ContentId<CollectionCandidateSearchInputArtifact>,
}

impl PreparedCollectionCandidateSearchInput {
    #[must_use]
    pub const fn input(&self) -> &CollectionCandidateSearchInputV1 {
        &self.input
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn id(&self) -> ContentId<CollectionCandidateSearchInputArtifact> {
        self.id
    }
}

/// Projects committed public authority fields into the first Candidate search input.
///
/// The local exploration input cannot substitute for a full admitted Oracle portfolio.
///
/// ```compile_fail
/// use cairn_migration::CollectionCandidateSearchInputV1;
/// use cairn_verification::AdmittedOracleV1;
/// fn require_full(_: AdmittedOracleV1) {}
/// fn invalid(local: CollectionCandidateSearchInputV1) { require_full(local); }
/// ```
///
/// # Errors
///
/// Rejects invalid authority bindings or canonical identity failures.
pub fn prepare_collection_candidate_search_input(
    authority: &CollectionCandidateSearchAuthorityInput,
) -> Result<PreparedCollectionCandidateSearchInput, CandidateSearchInputError> {
    let input = CollectionCandidateSearchInputV1 {
        schema_version: schema_v1(),
        task_id: authority.task_id,
        recovery_input: authority.recovery_input,
        intent_contract: authority.intent_contract,
        oracle_outcome: authority.oracle_outcome,
        oracle_claim: authority.oracle_claim,
        selection_claim: authority.selection_claim.clone(),
        domain: authority.domain,
        strength: authority.strength,
        scope: CollectionCandidateSearchScopeV1::LocalOracleExplorationOnly,
    };
    input.validate_authority(authority)?;
    let bytes = cairn_codec::to_vec(&input).map_err(codec)?;
    let id = ContentId::derive(&bytes).map_err(codec)?;
    Ok(PreparedCollectionCandidateSearchInput { input, bytes, id })
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CandidateSearchInputError {
    #[error("Candidate search input is invalid for the current local scope")]
    InvalidStructure,
    #[error("Candidate search input does not match committed public authority")]
    BindingMismatch,
    #[error("Candidate input codec failed: {0}")]
    Codec(String),
}

fn schema_v1() -> SchemaVersion {
    SchemaVersion::new(1).expect("current V1 is a valid non-zero schema version")
}

fn codec(error: impl std::fmt::Display) -> CandidateSearchInputError {
    CandidateSearchInputError::Codec(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id<T: ContentType>(label: &[u8]) -> ContentId<T> {
        ContentId::derive(label).expect("content identity")
    }

    fn authority() -> CollectionCandidateSearchAuthorityInput {
        CollectionCandidateSearchAuthorityInput::new(
            TaskId::new(),
            id::<IntentRecoveryInputArtifact>(b"recovery input"),
            id::<MigrationIntentContractArtifact>(b"intent contract"),
            id::<CollectionOracleAdmissionPublicOutcomeArtifact>(b"Oracle outcome"),
            id::<AdmittedCollectionOracleClaimArtifact>(b"Oracle claim"),
            SirCallerClaimId::new("copies-strictly-above").expect("selection claim"),
            CollectionOracleClaimDomainV1::FiniteNormalF32StrictlyAboveThreshold,
            CollectionOracleClaimStrengthV1::ExactOccurrenceMultisetAndReportedCount,
        )
    }

    #[test]
    fn local_candidate_input_is_strict_answer_free_and_authority_bound() {
        let source_authority = authority();
        let prepared =
            prepare_collection_candidate_search_input(&source_authority).expect("search input");
        assert_eq!(
            prepared.input().identity().expect("input id"),
            prepared.id()
        );
        assert_eq!(
            prepared.input().scope(),
            CollectionCandidateSearchScopeV1::LocalOracleExplorationOnly
        );
        let decoded: CollectionCandidateSearchInputV1 =
            cairn_codec::from_slice(prepared.bytes()).expect("strict round trip");
        let exact = CollectionCandidateSearchAuthorityInput::new(
            decoded.task_id(),
            decoded.recovery_input(),
            decoded.intent_contract(),
            decoded.oracle_outcome(),
            decoded.oracle_claim(),
            decoded.selection_claim().clone(),
            decoded.domain(),
            decoded.strength(),
        );
        decoded.validate_authority(&exact).expect("exact authority");
        let changed = authority();
        assert_eq!(
            decoded.validate_authority(&changed),
            Err(CandidateSearchInputError::BindingMismatch)
        );

        let serialized = String::from_utf8(prepared.bytes().to_vec()).expect("canonical JSON");
        for forbidden in [
            "qualification_receipt",
            "comparison",
            "execution_receipt",
            "executable",
            "expected",
        ] {
            assert!(!serialized.contains(forbidden), "leaked {forbidden}");
        }

        let mut invalid = serde_json::to_value(&decoded).expect("input JSON");
        invalid["schema_version"] = serde_json::json!(2);
        assert!(serde_json::from_value::<CollectionCandidateSearchInputV1>(invalid).is_err());
        let mut invalid = serde_json::to_value(&decoded).expect("input JSON");
        invalid["legacy_portfolio"] = serde_json::json!(true);
        assert!(serde_json::from_value::<CollectionCandidateSearchInputV1>(invalid).is_err());
    }
}
