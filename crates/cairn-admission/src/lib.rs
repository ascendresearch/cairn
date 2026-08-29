//! Deterministic Intent Admission types, promotion gate, and first contract-only Oracle consumer.

use std::{fmt, io::Cursor, str::FromStr};

use cairn_migration::{
    AdmittedCollectionOracleClaimArtifact, AdmittedCollectionOracleClaimV1,
    AssembledCollectionF32OracleCaseInput, CollectionCandidateSearchAuthorityInput,
    CollectionOracleAdmissionGateArtifact, CollectionOracleAdmissionPublicOutcomeArtifact,
    CollectionOracleClaimProposalArtifact, CollectionOracleQualificationExecution,
    CollectionOracleQualificationReceiptArtifact, CollectionOutputComparisonEvidenceArtifact,
    CollectionOutputOracleDecisionV1, CollectionOutputOraclePolicyV1,
    IntentHypothesisSetProposalV1, IntentRecoveryInputArtifact, IntentRecoveryInputV1,
    MigrationIntentContractArtifact, PreparedAdmittedCollectionOracleClaim,
    PreparedCollectionCandidateSearchInput, SirCallerClaimId, SirHypothesisId,
    SirIntentHypothesisSetProposalArtifact, UserIntentDecisionRequestArtifact,
    UserIntentDecisionRequestV1, collection_oracle_admission_gate_id,
    derive_user_intent_decision_requests,
    prepare_collection_candidate_search_input as prepare_candidate_input_mechanically,
};
use cairn_protocol::{ContentId, ContentType, SchemaVersion, TaskId};
use cairn_record::ContentStore;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

const MAX_AUTHORITY_SUBJECT_BYTES: usize = 128;
const INTENT_USER_DECISION_GATE_V1: &[u8] = include_bytes!("lib.rs");

/// Controller-published grant proving who may decide one exact intent scope.
pub enum UserIntentAuthorityGrantArtifact {}

impl ContentType for UserIntentAuthorityGrantArtifact {
    const DOMAIN: &'static str = "migration.user-intent-authority-grant.v1";
}

/// Exact user decision consumed by Intent Admission.
///
/// A request identity cannot substitute for an authority decision identity.
///
/// ```compile_fail
/// use cairn_admission::UserIntentDecisionArtifact;
/// use cairn_migration::UserIntentDecisionRequestArtifact;
/// use cairn_protocol::ContentId;
/// fn require_decision(_: ContentId<UserIntentDecisionArtifact>) {}
/// let request = ContentId::<UserIntentDecisionRequestArtifact>::derive(b"request").unwrap();
/// require_decision(request);
/// ```
pub enum UserIntentDecisionArtifact {}

impl ContentType for UserIntentDecisionArtifact {
    const DOMAIN: &'static str = "migration.user-intent-decision.v1";
}

/// Exact deterministic gate implementation used for this decision.
pub enum IntentUserDecisionGateArtifact {}

impl ContentType for IntentUserDecisionGateArtifact {
    const DOMAIN: &'static str = "admission.intent-user-decision-gate.v1";
}

/// Full Admission-owned decision committed before public publication.
pub enum RestrictedIntentAdmissionDecisionArtifact {}

impl ContentType for RestrictedIntentAdmissionDecisionArtifact {
    const DOMAIN: &'static str = "admission.intent-decision-restricted.v1";
}

/// Authenticated application subject selected by a Controller authority grant.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TaskIntentAuthoritySubject(String);

impl TaskIntentAuthoritySubject {
    /// Creates a bounded printable subject identifier.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversized, whitespace-containing, or control-containing identifiers.
    pub fn new(value: impl Into<String>) -> Result<Self, IntentPromotionError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_AUTHORITY_SUBJECT_BYTES
            || !value.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(IntentPromotionError::InvalidStructure(
                "task intent authority subject",
            ));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskIntentAuthoritySubject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for TaskIntentAuthoritySubject {
    type Err = IntentPromotionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl Serialize for TaskIntentAuthoritySubject {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for TaskIntentAuthoritySubject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

/// Claim-scoped authority granted by the Controller after authenticating the task authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum UserIntentAuthorityScopeV1 {
    CollectionOutput { selection_claim: SirCallerClaimId },
}

/// Exact Controller-published authority grant.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UserIntentAuthorityGrantV1 {
    schema_version: SchemaVersion,
    task_id: TaskId,
    subject: TaskIntentAuthoritySubject,
    scope: UserIntentAuthorityScopeV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UserIntentAuthorityGrantWire {
    schema_version: SchemaVersion,
    task_id: TaskId,
    subject: TaskIntentAuthoritySubject,
    scope: UserIntentAuthorityScopeV1,
}

impl UserIntentAuthorityGrantV1 {
    #[must_use]
    pub fn new(
        task_id: TaskId,
        subject: TaskIntentAuthoritySubject,
        scope: UserIntentAuthorityScopeV1,
    ) -> Self {
        Self {
            schema_version: schema_v1(),
            task_id,
            subject,
            scope,
        }
    }

    /// Derives the exact grant identity.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or typed identity derivation fails.
    pub fn identity(
        &self,
    ) -> Result<ContentId<UserIntentAuthorityGrantArtifact>, IntentPromotionError> {
        derive_id(self)
    }
}

impl TryFrom<UserIntentAuthorityGrantWire> for UserIntentAuthorityGrantV1 {
    type Error = IntentPromotionError;

    fn try_from(wire: UserIntentAuthorityGrantWire) -> Result<Self, Self::Error> {
        if wire.schema_version != schema_v1() {
            return Err(IntentPromotionError::InvalidStructure(
                "user intent authority grant schema",
            ));
        }
        Ok(Self::new(wire.task_id, wire.subject, wire.scope))
    }
}

impl<'de> Deserialize<'de> for UserIntentAuthorityGrantV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        UserIntentAuthorityGrantWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Membership requirement for collection-like outputs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionMembershipContractV1 {
    ExactSelectedOccurrences,
}

/// Reported cardinality requirement for collection-like outputs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionReportedCountContractV1 {
    ExactSelectedOccurrenceCount,
}

/// Whether sequence position is part of a collection-output contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionOutputOrderContractV1 {
    UnspecifiedPermutation,
    StableInputRelative,
}

/// Machine-readable desired semantics supplied by the actual task authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionOutputIntentV1 {
    selection_claim: SirCallerClaimId,
    membership: CollectionMembershipContractV1,
    reported_count: CollectionReportedCountContractV1,
    order: CollectionOutputOrderContractV1,
}

impl CollectionOutputIntentV1 {
    #[must_use]
    pub const fn exact_selected_occurrences(
        selection_claim: SirCallerClaimId,
        order: CollectionOutputOrderContractV1,
    ) -> Self {
        Self {
            selection_claim,
            membership: CollectionMembershipContractV1::ExactSelectedOccurrences,
            reported_count: CollectionReportedCountContractV1::ExactSelectedOccurrenceCount,
            order,
        }
    }
}

/// Current claim families that an actual task authority may state directly.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "contract", rename_all = "kebab-case")]
pub enum AuthoritativeIntentClaimV1 {
    CollectionOutput(CollectionOutputIntentV1),
}

/// Exact response to one scoped user-decision request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum UserIntentDecisionResponseV1 {
    SelectHypothesis {
        hypothesis: SirHypothesisId,
        authoritative_claim: AuthoritativeIntentClaimV1,
    },
    KeepUnknown,
    ProvideAuthoritativeClaim {
        authoritative_claim: AuthoritativeIntentClaimV1,
    },
}

/// User answer bound to an exact request and Controller authority grant.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UserIntentDecisionV1 {
    schema_version: SchemaVersion,
    request: ContentId<UserIntentDecisionRequestArtifact>,
    authority_grant: ContentId<UserIntentAuthorityGrantArtifact>,
    response: UserIntentDecisionResponseV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UserIntentDecisionWire {
    schema_version: SchemaVersion,
    request: ContentId<UserIntentDecisionRequestArtifact>,
    authority_grant: ContentId<UserIntentAuthorityGrantArtifact>,
    response: UserIntentDecisionResponseV1,
}

impl UserIntentDecisionV1 {
    #[must_use]
    pub fn new(
        request: ContentId<UserIntentDecisionRequestArtifact>,
        authority_grant: ContentId<UserIntentAuthorityGrantArtifact>,
        response: UserIntentDecisionResponseV1,
    ) -> Self {
        Self {
            schema_version: schema_v1(),
            request,
            authority_grant,
            response,
        }
    }

    /// Derives the exact decision identity.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or typed identity derivation fails.
    pub fn identity(&self) -> Result<ContentId<UserIntentDecisionArtifact>, IntentPromotionError> {
        derive_id(self)
    }

    #[must_use]
    pub const fn request(&self) -> ContentId<UserIntentDecisionRequestArtifact> {
        self.request
    }

    #[must_use]
    pub const fn authority_grant(&self) -> ContentId<UserIntentAuthorityGrantArtifact> {
        self.authority_grant
    }
}

impl TryFrom<UserIntentDecisionWire> for UserIntentDecisionV1 {
    type Error = IntentPromotionError;

    fn try_from(wire: UserIntentDecisionWire) -> Result<Self, Self::Error> {
        if wire.schema_version != schema_v1() {
            return Err(IntentPromotionError::InvalidStructure(
                "user intent decision schema",
            ));
        }
        Ok(Self::new(wire.request, wire.authority_grant, wire.response))
    }
}

impl<'de> Deserialize<'de> for UserIntentDecisionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        UserIntentDecisionWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Immutable first admitted intent contract.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MigrationIntentContractV1 {
    schema_version: SchemaVersion,
    task_id: TaskId,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
    request: ContentId<UserIntentDecisionRequestArtifact>,
    authority_grant: ContentId<UserIntentAuthorityGrantArtifact>,
    user_decision: ContentId<UserIntentDecisionArtifact>,
    selected_hypothesis: Option<SirHypothesisId>,
    admitted_claim: AuthoritativeIntentClaimV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationIntentContractWire {
    schema_version: SchemaVersion,
    task_id: TaskId,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
    request: ContentId<UserIntentDecisionRequestArtifact>,
    authority_grant: ContentId<UserIntentAuthorityGrantArtifact>,
    user_decision: ContentId<UserIntentDecisionArtifact>,
    selected_hypothesis: Option<SirHypothesisId>,
    admitted_claim: AuthoritativeIntentClaimV1,
}

impl MigrationIntentContractV1 {
    fn validate(&self) -> Result<(), IntentPromotionError> {
        if self.schema_version != schema_v1() {
            return Err(IntentPromotionError::InvalidStructure(
                "migration intent contract schema",
            ));
        }
        Ok(())
    }

    /// Derives the exact admitted contract identity.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or typed identity derivation fails.
    pub fn identity(
        &self,
    ) -> Result<ContentId<MigrationIntentContractArtifact>, IntentPromotionError> {
        derive_id(self)
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn recovery_input(&self) -> ContentId<IntentRecoveryInputArtifact> {
        self.recovery_input
    }
}

impl TryFrom<MigrationIntentContractWire> for MigrationIntentContractV1 {
    type Error = IntentPromotionError;

    fn try_from(wire: MigrationIntentContractWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            task_id: wire.task_id,
            recovery_input: wire.recovery_input,
            proposal: wire.proposal,
            request: wire.request,
            authority_grant: wire.authority_grant,
            user_decision: wire.user_decision,
            selected_hypothesis: wire.selected_hypothesis,
            admitted_claim: wire.admitted_claim,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for MigrationIntentContractV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        MigrationIntentContractWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Full deterministic decision stored only under Admission authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RestrictedIntentAdmissionDecisionV1 {
    schema_version: SchemaVersion,
    mechanism: ContentId<IntentUserDecisionGateArtifact>,
    user_decision: ContentId<UserIntentDecisionArtifact>,
    contract: ContentId<MigrationIntentContractArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestrictedIntentAdmissionDecisionWire {
    schema_version: SchemaVersion,
    mechanism: ContentId<IntentUserDecisionGateArtifact>,
    user_decision: ContentId<UserIntentDecisionArtifact>,
    contract: ContentId<MigrationIntentContractArtifact>,
}

impl RestrictedIntentAdmissionDecisionV1 {
    /// Validates and derives the exact restricted decision identity.
    ///
    /// # Errors
    ///
    /// Rejects a non-V1 envelope, wrong gate identity, or codec/identity failure.
    pub fn identity(
        &self,
    ) -> Result<ContentId<RestrictedIntentAdmissionDecisionArtifact>, IntentPromotionError> {
        if self.schema_version != schema_v1() || self.mechanism != intent_user_decision_gate_id()? {
            return Err(IntentPromotionError::InvalidStructure(
                "restricted intent admission decision",
            ));
        }
        derive_id(self)
    }
}

impl TryFrom<RestrictedIntentAdmissionDecisionWire> for RestrictedIntentAdmissionDecisionV1 {
    type Error = IntentPromotionError;

    fn try_from(wire: RestrictedIntentAdmissionDecisionWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            mechanism: wire.mechanism,
            user_decision: wire.user_decision,
            contract: wire.contract,
        };
        let _ = value.identity()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for RestrictedIntentAdmissionDecisionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RestrictedIntentAdmissionDecisionWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Public publication emitted only after the restricted decision is archived.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentAdmissionPublicOutcomeV1 {
    schema_version: SchemaVersion,
    contract: MigrationIntentContractV1,
    restricted_decision: ContentId<RestrictedIntentAdmissionDecisionArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentAdmissionPublicOutcomeWire {
    schema_version: SchemaVersion,
    contract: MigrationIntentContractV1,
    restricted_decision: ContentId<RestrictedIntentAdmissionDecisionArtifact>,
}

impl IntentAdmissionPublicOutcomeV1 {
    fn validate(&self) -> Result<(), IntentPromotionError> {
        if self.schema_version != schema_v1() {
            return Err(IntentPromotionError::InvalidStructure(
                "intent admission public outcome schema",
            ));
        }
        self.contract.validate()
    }

    #[must_use]
    pub const fn contract(&self) -> &MigrationIntentContractV1 {
        &self.contract
    }

    #[must_use]
    pub const fn restricted_decision(
        &self,
    ) -> ContentId<RestrictedIntentAdmissionDecisionArtifact> {
        self.restricted_decision
    }
}

impl TryFrom<IntentAdmissionPublicOutcomeWire> for IntentAdmissionPublicOutcomeV1 {
    type Error = IntentPromotionError;

    fn try_from(wire: IntentAdmissionPublicOutcomeWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            contract: wire.contract,
            restricted_decision: wire.restricted_decision,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for IntentAdmissionPublicOutcomeV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentAdmissionPublicOutcomeWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Prepared deterministic gate result; the process must archive the restricted decision before
/// publishing `public_outcome`.
pub struct PreparedIntentAdmissionV1 {
    restricted_decision: RestrictedIntentAdmissionDecisionV1,
    public_outcome: IntentAdmissionPublicOutcomeV1,
}

impl PreparedIntentAdmissionV1 {
    #[must_use]
    pub const fn restricted_decision(&self) -> &RestrictedIntentAdmissionDecisionV1 {
        &self.restricted_decision
    }

    #[must_use]
    pub const fn public_outcome(&self) -> &IntentAdmissionPublicOutcomeV1 {
        &self.public_outcome
    }
}

/// Mechanically promotes one exact task-authority decision.
///
/// # Errors
///
/// Fails closed on any identity, task, request, option, authority-scope, or caller-claim mismatch.
#[allow(clippy::too_many_arguments)]
pub fn promote_user_intent(
    proposal_id: ContentId<SirIntentHypothesisSetProposalArtifact>,
    proposal: &IntentHypothesisSetProposalV1,
    recovery_input_id: ContentId<IntentRecoveryInputArtifact>,
    recovery_input: &IntentRecoveryInputV1,
    request_id: ContentId<UserIntentDecisionRequestArtifact>,
    request: &UserIntentDecisionRequestV1,
    grant_id: ContentId<UserIntentAuthorityGrantArtifact>,
    grant: &UserIntentAuthorityGrantV1,
    decision_id: ContentId<UserIntentDecisionArtifact>,
    decision: &UserIntentDecisionV1,
) -> Result<PreparedIntentAdmissionV1, IntentPromotionError> {
    require_identity(grant.identity()?, grant_id, "authority grant")?;
    require_identity(decision.identity()?, decision_id, "user decision")?;
    require_identity(
        request.identity().map_err(migration_error)?,
        request_id,
        "decision request",
    )?;
    if decision.request != request_id || decision.authority_grant != grant_id {
        return Err(IntentPromotionError::Binding("decision input"));
    }
    if grant.task_id != recovery_input.task_id()
        || request.proposal() != proposal_id
        || request.recovery_input() != recovery_input_id
    {
        return Err(IntentPromotionError::Binding("task/proposal/request"));
    }
    let derived = derive_user_intent_decision_requests(
        proposal_id,
        proposal,
        recovery_input_id,
        recovery_input,
    )
    .map_err(migration_error)?;
    if !derived.requests().iter().any(|candidate| {
        candidate
            .identity()
            .is_ok_and(|candidate_id| candidate_id == request_id && candidate == request)
    }) {
        return Err(IntentPromotionError::Binding("derived decision request"));
    }

    let (selected_hypothesis, admitted_claim) = match &decision.response {
        UserIntentDecisionResponseV1::SelectHypothesis {
            hypothesis,
            authoritative_claim,
        } => {
            if !request
                .options()
                .iter()
                .any(|option| option.hypothesis() == hypothesis)
            {
                return Err(IntentPromotionError::UnofferedHypothesis);
            }
            (Some(hypothesis.clone()), authoritative_claim.clone())
        }
        UserIntentDecisionResponseV1::ProvideAuthoritativeClaim {
            authoritative_claim,
        } => (None, authoritative_claim.clone()),
        UserIntentDecisionResponseV1::KeepUnknown => {
            return Err(IntentPromotionError::KeptUnknown);
        }
    };
    validate_authority_scope(grant, recovery_input, &admitted_claim)?;

    let contract = MigrationIntentContractV1 {
        schema_version: schema_v1(),
        task_id: recovery_input.task_id(),
        recovery_input: recovery_input_id,
        proposal: proposal_id,
        request: request_id,
        authority_grant: grant_id,
        user_decision: decision_id,
        selected_hypothesis,
        admitted_claim,
    };
    contract.validate()?;
    let restricted_decision = RestrictedIntentAdmissionDecisionV1 {
        schema_version: schema_v1(),
        mechanism: intent_user_decision_gate_id()?,
        user_decision: decision_id,
        contract: contract.identity()?,
    };
    let restricted_decision_id = restricted_decision.identity()?;
    Ok(PreparedIntentAdmissionV1 {
        public_outcome: IntentAdmissionPublicOutcomeV1 {
            schema_version: schema_v1(),
            contract,
            restricted_decision: restricted_decision_id,
        },
        restricted_decision,
    })
}

fn validate_authority_scope(
    grant: &UserIntentAuthorityGrantV1,
    recovery_input: &IntentRecoveryInputV1,
    admitted_claim: &AuthoritativeIntentClaimV1,
) -> Result<(), IntentPromotionError> {
    let (
        UserIntentAuthorityScopeV1::CollectionOutput { selection_claim },
        AuthoritativeIntentClaimV1::CollectionOutput(contract),
    ) = (&grant.scope, admitted_claim);
    if selection_claim != &contract.selection_claim
        || !recovery_input
            .request()
            .caller()
            .claims()
            .iter()
            .any(|claim| claim.id() == selection_claim)
    {
        return Err(IntentPromotionError::AuthorityScope);
    }
    Ok(())
}

/// Exact identity of the deterministic gate implementation used by this slice.
///
/// # Errors
///
/// Returns an error only if typed identity derivation fails.
pub fn intent_user_decision_gate_id()
-> Result<ContentId<IntentUserDecisionGateArtifact>, IntentPromotionError> {
    ContentId::derive(INTENT_USER_DECISION_GATE_V1)
        .map_err(|error| IntentPromotionError::Codec(error.to_string()))
}

/// Derives the first real Oracle comparator decision only from a public admitted outcome.
///
/// A proposal cannot be substituted for the admitted outcome.
///
/// ```compile_fail
/// use cairn_admission::derive_collection_output_oracle_decision;
/// use cairn_migration::IntentHypothesisSetProposalV1;
/// fn invalid(proposal: &IntentHypothesisSetProposalV1) {
///     let _ = derive_collection_output_oracle_decision(proposal);
/// }
/// ```
///
/// # Errors
///
/// Rejects a malformed outcome or a contract without collection-output semantics.
pub fn derive_collection_output_oracle_decision(
    outcome: &IntentAdmissionPublicOutcomeV1,
) -> Result<CollectionOutputOracleDecisionV1, IntentPromotionError> {
    if outcome.schema_version != schema_v1() {
        return Err(IntentPromotionError::InvalidStructure(
            "intent admission public outcome",
        ));
    }
    let AuthoritativeIntentClaimV1::CollectionOutput(contract) = &outcome.contract.admitted_claim;
    let policy = match contract.order {
        CollectionOutputOrderContractV1::UnspecifiedPermutation => {
            CollectionOutputOraclePolicyV1::ExactMultisetAndCount
        }
        CollectionOutputOrderContractV1::StableInputRelative => {
            CollectionOutputOraclePolicyV1::ExactSequenceAndCount
        }
    };
    Ok(CollectionOutputOracleDecisionV1::new(
        outcome.contract.identity()?,
        contract.selection_claim.clone(),
        policy,
    ))
}

/// Exact restricted Oracle Admission decision committed before its public outcome.
pub enum RestrictedCollectionOracleAdmissionDecisionArtifact {}

impl ContentType for RestrictedCollectionOracleAdmissionDecisionArtifact {
    const DOMAIN: &'static str = "migration.oracle-collection-admission-decision-restricted.v1";
}

/// Restricted decision binding the exact admitted intent and qualified local claim.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RestrictedCollectionOracleAdmissionDecisionV1 {
    schema_version: SchemaVersion,
    gate: ContentId<CollectionOracleAdmissionGateArtifact>,
    intent_restricted_decision: ContentId<RestrictedIntentAdmissionDecisionArtifact>,
    qualification_receipt: ContentId<CollectionOracleQualificationReceiptArtifact>,
    claim: ContentId<AdmittedCollectionOracleClaimArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestrictedCollectionOracleAdmissionDecisionWire {
    schema_version: SchemaVersion,
    gate: ContentId<CollectionOracleAdmissionGateArtifact>,
    intent_restricted_decision: ContentId<RestrictedIntentAdmissionDecisionArtifact>,
    qualification_receipt: ContentId<CollectionOracleQualificationReceiptArtifact>,
    claim: ContentId<AdmittedCollectionOracleClaimArtifact>,
}

impl RestrictedCollectionOracleAdmissionDecisionV1 {
    fn validate(&self) -> Result<(), IntentPromotionError> {
        if self.schema_version != schema_v1()
            || self.gate != collection_oracle_admission_gate_id().map_err(migration_error)?
        {
            return Err(IntentPromotionError::InvalidStructure(
                "restricted collection Oracle admission decision",
            ));
        }
        Ok(())
    }

    /// Derives the exact restricted decision identity.
    ///
    /// # Errors
    ///
    /// Rejects non-V1, stale-gate, codec, or identity material.
    pub fn identity(
        &self,
    ) -> Result<ContentId<RestrictedCollectionOracleAdmissionDecisionArtifact>, IntentPromotionError>
    {
        self.validate()?;
        derive_id(self)
    }
}

impl TryFrom<RestrictedCollectionOracleAdmissionDecisionWire>
    for RestrictedCollectionOracleAdmissionDecisionV1
{
    type Error = IntentPromotionError;

    fn try_from(
        wire: RestrictedCollectionOracleAdmissionDecisionWire,
    ) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            gate: wire.gate,
            intent_restricted_decision: wire.intent_restricted_decision,
            qualification_receipt: wire.qualification_receipt,
            claim: wire.claim,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for RestrictedCollectionOracleAdmissionDecisionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RestrictedCollectionOracleAdmissionDecisionWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Minimal public outcome for one published local Oracle claim.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CollectionOracleAdmissionPublicOutcomeV1 {
    schema_version: SchemaVersion,
    intent_contract: MigrationIntentContractV1,
    claim: AdmittedCollectionOracleClaimV1,
    restricted_decision: ContentId<RestrictedCollectionOracleAdmissionDecisionArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionOracleAdmissionPublicOutcomeWire {
    schema_version: SchemaVersion,
    intent_contract: MigrationIntentContractV1,
    claim: AdmittedCollectionOracleClaimV1,
    restricted_decision: ContentId<RestrictedCollectionOracleAdmissionDecisionArtifact>,
}

impl CollectionOracleAdmissionPublicOutcomeV1 {
    fn validate(&self) -> Result<(), IntentPromotionError> {
        if self.schema_version != schema_v1()
            || self.claim.contract() != self.intent_contract.identity()?
        {
            return Err(IntentPromotionError::InvalidStructure(
                "collection Oracle admission public outcome",
            ));
        }
        Ok(())
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.intent_contract.task_id()
    }

    #[must_use]
    pub const fn recovery_input(&self) -> ContentId<IntentRecoveryInputArtifact> {
        self.intent_contract.recovery_input()
    }

    #[must_use]
    pub const fn intent_contract(&self) -> &MigrationIntentContractV1 {
        &self.intent_contract
    }

    /// Derives the exact admitted intent contract identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the embedded public contract is invalid.
    pub fn intent_contract_id(
        &self,
    ) -> Result<ContentId<MigrationIntentContractArtifact>, IntentPromotionError> {
        self.intent_contract.identity()
    }

    #[must_use]
    pub const fn claim(&self) -> &AdmittedCollectionOracleClaimV1 {
        &self.claim
    }

    #[must_use]
    pub const fn restricted_decision(
        &self,
    ) -> ContentId<RestrictedCollectionOracleAdmissionDecisionArtifact> {
        self.restricted_decision
    }

    /// Derives the exact public outcome identity.
    ///
    /// # Errors
    ///
    /// Rejects invalid bindings or codec/identity material.
    pub fn identity(
        &self,
    ) -> Result<ContentId<CollectionOracleAdmissionPublicOutcomeArtifact>, IntentPromotionError>
    {
        self.validate()?;
        derive_id(self)
    }
}

impl TryFrom<CollectionOracleAdmissionPublicOutcomeWire>
    for CollectionOracleAdmissionPublicOutcomeV1
{
    type Error = IntentPromotionError;

    fn try_from(wire: CollectionOracleAdmissionPublicOutcomeWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            intent_contract: wire.intent_contract,
            claim: wire.claim,
            restricted_decision: wire.restricted_decision,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CollectionOracleAdmissionPublicOutcomeV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CollectionOracleAdmissionPublicOutcomeWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Prepared local Oracle admission. Its public outcome must not be published before commit.
pub struct PreparedCollectionOracleAdmissionV1 {
    claim: PreparedAdmittedCollectionOracleClaim,
    restricted_decision: RestrictedCollectionOracleAdmissionDecisionV1,
    restricted_decision_bytes: Vec<u8>,
    restricted_decision_id: ContentId<RestrictedCollectionOracleAdmissionDecisionArtifact>,
    public_outcome: CollectionOracleAdmissionPublicOutcomeV1,
}

impl PreparedCollectionOracleAdmissionV1 {
    #[must_use]
    pub const fn claim_material(&self) -> &PreparedAdmittedCollectionOracleClaim {
        &self.claim
    }

    #[must_use]
    pub const fn restricted_decision(&self) -> &RestrictedCollectionOracleAdmissionDecisionV1 {
        &self.restricted_decision
    }

    #[must_use]
    pub const fn restricted_decision_id(
        &self,
    ) -> ContentId<RestrictedCollectionOracleAdmissionDecisionArtifact> {
        self.restricted_decision_id
    }
}

/// Freezes the first local Oracle claim only from an already admitted public intent outcome and
/// independently validated honest/fault execution controls.
///
/// A local proposal cannot substitute for the admitted intent outcome.
///
/// ```compile_fail
/// use cairn_admission::admit_collection_oracle_claim;
/// use cairn_migration::{
///     AssembledCollectionF32OracleCaseInput, CollectionOracleClaimProposalV1,
///     CollectionOracleQualificationExecution,
/// };
/// use cairn_record::ContentStore;
/// fn invalid<C1: ContentStore, C2: ContentStore>(
///     proposal: &CollectionOracleClaimProposalV1,
///     case: &AssembledCollectionF32OracleCaseInput,
///     honest: &CollectionOracleQualificationExecution<'_, C1>,
///     fault: &CollectionOracleQualificationExecution<'_, C2>,
/// ) {
///     let _ = admit_collection_oracle_claim(proposal, case, honest, fault);
/// }
/// ```
///
/// Constructing the returned value does not publish authority. Call
/// [`commit_collection_oracle_admission`] before exposing its public outcome.
///
/// # Errors
///
/// Rejects a malformed admitted outcome or any failed qualification/binding control.
pub fn admit_collection_oracle_claim<C1: ContentStore, C2: ContentStore>(
    outcome: &IntentAdmissionPublicOutcomeV1,
    case: &AssembledCollectionF32OracleCaseInput,
    honest: &CollectionOracleQualificationExecution<'_, C1>,
    fault: &CollectionOracleQualificationExecution<'_, C2>,
) -> Result<PreparedCollectionOracleAdmissionV1, IntentPromotionError> {
    let decision = derive_collection_output_oracle_decision(outcome)?;
    let claim =
        cairn_migration::prepare_admitted_collection_oracle_claim(&decision, case, honest, fault)
            .map_err(migration_error)?;
    let contract_id = outcome.contract().identity()?;
    if claim.claim().contract() != contract_id
        || claim.claim().decision() != decision.identity().map_err(migration_error)?
        || claim.claim().selection_claim() != decision.selection_claim()
    {
        return Err(IntentPromotionError::Binding(
            "qualified Oracle claim does not match admitted intent",
        ));
    }
    let restricted_decision = RestrictedCollectionOracleAdmissionDecisionV1 {
        schema_version: schema_v1(),
        gate: collection_oracle_admission_gate_id().map_err(migration_error)?,
        intent_restricted_decision: outcome.restricted_decision(),
        qualification_receipt: claim.receipt_id(),
        claim: claim.claim_id(),
    };
    let restricted_decision_bytes = cairn_codec::to_vec(&restricted_decision)?;
    let restricted_decision_id = restricted_decision.identity()?;
    let public_outcome = CollectionOracleAdmissionPublicOutcomeV1 {
        schema_version: schema_v1(),
        intent_contract: outcome.contract().clone(),
        claim: claim.claim().clone(),
        restricted_decision: restricted_decision_id,
    };
    public_outcome.validate()?;
    Ok(PreparedCollectionOracleAdmissionV1 {
        claim,
        restricted_decision,
        restricted_decision_bytes,
        restricted_decision_id,
        public_outcome,
    })
}

/// Commits every restricted artifact before returning the public Oracle outcome.
///
/// A raw local claim cannot substitute for the prepared Admission result.
///
/// ```compile_fail
/// use cairn_admission::commit_collection_oracle_admission;
/// use cairn_migration::AdmittedCollectionOracleClaimV1;
/// use cairn_record::ContentStore;
/// fn invalid<C: ContentStore>(store: &mut C, claim: &AdmittedCollectionOracleClaimV1) {
///     let _ = commit_collection_oracle_admission(store, claim);
/// }
/// ```
///
/// # Errors
///
/// Returns no public outcome if any restricted write or exact identity check fails.
pub fn commit_collection_oracle_admission<C: ContentStore>(
    restricted: &mut C,
    prepared: &PreparedCollectionOracleAdmissionV1,
) -> Result<CollectionOracleAdmissionPublicOutcomeV1, IntentPromotionError> {
    let claim = prepared.claim_material();
    archive_exact::<CollectionOracleClaimProposalArtifact>(
        restricted,
        claim.proposal_bytes(),
        claim.proposal_id(),
        "collection Oracle claim proposal",
    )?;
    archive_exact::<CollectionOutputComparisonEvidenceArtifact>(
        restricted,
        claim.honest_comparison().bytes(),
        claim.honest_comparison().id(),
        "honest collection Oracle comparison",
    )?;
    archive_exact::<CollectionOutputComparisonEvidenceArtifact>(
        restricted,
        claim.fault_comparison().bytes(),
        claim.fault_comparison().id(),
        "fault collection Oracle comparison",
    )?;
    archive_exact::<CollectionOracleQualificationReceiptArtifact>(
        restricted,
        claim.receipt_bytes(),
        claim.receipt_id(),
        "collection Oracle qualification receipt",
    )?;
    archive_exact::<AdmittedCollectionOracleClaimArtifact>(
        restricted,
        claim.claim_bytes(),
        claim.claim_id(),
        "admitted collection Oracle claim",
    )?;
    archive_exact::<RestrictedCollectionOracleAdmissionDecisionArtifact>(
        restricted,
        &prepared.restricted_decision_bytes,
        prepared.restricted_decision_id,
        "restricted collection Oracle admission decision",
    )?;
    prepared.public_outcome.validate()?;
    Ok(prepared.public_outcome.clone())
}

/// Projects a committed public Oracle outcome into the first local-only Candidate search input.
///
/// A raw admitted claim cannot substitute for a public outcome returned after restricted commit.
///
/// ```compile_fail
/// use cairn_admission::prepare_collection_candidate_search_input;
/// use cairn_migration::AdmittedCollectionOracleClaimV1;
/// fn invalid(claim: &AdmittedCollectionOracleClaimV1) {
///     let _ = prepare_collection_candidate_search_input(claim);
/// }
/// ```
///
/// # Errors
///
/// Rejects an invalid public binding or canonical Candidate input failure.
pub fn prepare_collection_candidate_search_input(
    outcome: &CollectionOracleAdmissionPublicOutcomeV1,
) -> Result<PreparedCollectionCandidateSearchInput, IntentPromotionError> {
    outcome.validate()?;
    let authority = CollectionCandidateSearchAuthorityInput::new(
        outcome.task_id(),
        outcome.recovery_input(),
        outcome.intent_contract_id()?,
        outcome.identity()?,
        outcome.claim().identity().map_err(migration_error)?,
        outcome.claim().selection_claim().clone(),
        outcome.claim().domain(),
        outcome.claim().strength(),
    );
    prepare_candidate_input_mechanically(&authority).map_err(migration_error)
}

/// Fail-closed errors from typed user decision promotion.
#[derive(Debug, Error)]
pub enum IntentPromotionError {
    #[error("canonical codec rejected Intent Admission material: {0}")]
    Codec(String),
    #[error("invalid Intent Admission structure: {0}")]
    InvalidStructure(&'static str),
    #[error("{0} identity does not match canonical bytes")]
    IdentityMismatch(&'static str),
    #[error("Intent Admission binding mismatch: {0}")]
    Binding(&'static str),
    #[error("selected hypothesis was not offered by the exact decision request")]
    UnofferedHypothesis,
    #[error("the task authority kept the desired semantic claim unknown")]
    KeptUnknown,
    #[error("the authority grant does not cover the authoritative claim scope")]
    AuthorityScope,
    #[error("migration contract rejected: {0}")]
    Migration(String),
    #[error("Intent Admission restricted storage failed: {0}")]
    Storage(String),
}

impl From<cairn_codec::CodecError> for IntentPromotionError {
    fn from(error: cairn_codec::CodecError) -> Self {
        Self::Codec(error.to_string())
    }
}

fn derive_id<T: ContentType>(value: &impl Serialize) -> Result<ContentId<T>, IntentPromotionError> {
    let bytes = cairn_codec::to_vec(value)?;
    ContentId::derive(&bytes).map_err(|error| IntentPromotionError::Codec(error.to_string()))
}

fn schema_v1() -> SchemaVersion {
    SchemaVersion::new(1).expect("current V1 is a valid non-zero schema version")
}

fn require_identity<T: ContentType>(
    actual: ContentId<T>,
    expected: ContentId<T>,
    name: &'static str,
) -> Result<(), IntentPromotionError> {
    if actual != expected {
        return Err(IntentPromotionError::IdentityMismatch(name));
    }
    Ok(())
}

fn archive_exact<T: ContentType>(
    store: &mut impl ContentStore,
    bytes: &[u8],
    expected: ContentId<T>,
    name: &'static str,
) -> Result<(), IntentPromotionError> {
    let archived = store
        .put::<T>(&mut Cursor::new(bytes))
        .map_err(|error| IntentPromotionError::Storage(error.to_string()))?
        .content_id;
    require_identity(archived, expected, name)
}

fn migration_error(error: impl fmt::Display) -> IntentPromotionError {
    IntentPromotionError::Migration(error.to_string())
}

#[cfg(test)]
mod oracle_publication_tests {
    use std::io::{Read, Write};

    use super::*;
    use cairn_execution::{
        CapturedOutput, DiagnosticByteLimit, EvidenceByteLimit, ExecutionBackend, ExecutionCapture,
        ExecutionCompletion, ExecutionElapsedMillis, ExecutionEnvironmentArtifact, ExecutionInput,
        ExecutionOutcome, InputBundleArtifact, OutputByteLimit, ResolvedProgramIdentity,
        ScriptedExecutor, TrustedExecutionEvidence, authorize_execution_attempt,
        begin_execution_attempt, execute_execution_attempt, prepare_execution_job,
    };
    use cairn_migration::{
        CallAdapterCaptureLimits, CallAdapterCompletionV1, CallAdapterExecutableByteLimit,
        CallAdapterObservedOutputV1, CallAdapterResultV1, CollectionF32Bits,
        CollectionOracleQualificationExecution, MigrationExecutionNeed, MigrationValidationTier,
        PreparedCallAdapterInput, PreparedCallAdapterJob, ValidatedCallAdapterExecution,
        assemble_collection_f32_oracle_case, compose_call_adapter_job,
        prepare_collection_output_call_adapter_input,
        validate_collection_output_call_adapter_receipt,
    };
    use cairn_protocol::{AttemptId, CommandId, JobId, ObservedAtUnixMillis};
    use cairn_record::{ContentDescriptor, ContentStoreError};
    use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

    const TEST_BACKEND: &str = "synthetic-collection-publication-v1";

    struct CompletedControl {
        _directory: tempfile::TempDir,
        content: SqliteContentStore,
        adapter: PreparedCallAdapterInput,
        execution: ValidatedCallAdapterExecution,
    }

    fn id<T: ContentType>(label: &[u8]) -> ContentId<T> {
        ContentId::derive(label).expect("content identity")
    }

    fn intent_outcome() -> IntentAdmissionPublicOutcomeV1 {
        let selection_claim = SirCallerClaimId::new("copies-strictly-above").expect("claim");
        let contract = MigrationIntentContractV1 {
            schema_version: schema_v1(),
            task_id: TaskId::new(),
            recovery_input: id::<IntentRecoveryInputArtifact>(b"recovery input"),
            proposal: id::<SirIntentHypothesisSetProposalArtifact>(b"proposal"),
            request: id::<UserIntentDecisionRequestArtifact>(b"request"),
            authority_grant: id::<UserIntentAuthorityGrantArtifact>(b"grant"),
            user_decision: id::<UserIntentDecisionArtifact>(b"decision"),
            selected_hypothesis: None,
            admitted_claim: AuthoritativeIntentClaimV1::CollectionOutput(
                CollectionOutputIntentV1::exact_selected_occurrences(
                    selection_claim,
                    CollectionOutputOrderContractV1::UnspecifiedPermutation,
                ),
            ),
        };
        IntentAdmissionPublicOutcomeV1 {
            schema_version: schema_v1(),
            contract,
            restricted_decision: id::<RestrictedIntentAdmissionDecisionArtifact>(
                b"restricted intent decision",
            ),
        }
    }

    fn f32_bits(value: f32) -> CollectionF32Bits {
        CollectionF32Bits::new(value.to_bits()).expect("finite normal non-zero f32")
    }

    fn complete_control(
        case: &AssembledCollectionF32OracleCaseInput,
        executable: &[u8],
        selected: &[f32],
    ) -> CompletedControl {
        let adapter = prepare_collection_output_call_adapter_input(
            case,
            executable,
            CallAdapterExecutableByteLimit::new(
                u64::try_from(executable.len()).expect("executable length"),
            )
            .expect("executable limit"),
        )
        .expect("adapter input");
        let directory = tempfile::tempdir().expect("execution state");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content store");
        let mut events =
            SqliteEventStore::open(directory.path().join("events.db")).expect("event store");
        assert_eq!(
            content
                .put::<InputBundleArtifact>(&mut Cursor::new(adapter.input_bundle_bytes()))
                .expect("input bundle")
                .content_id,
            adapter.input_bundle_id()
        );
        let environment = content
            .put::<ExecutionEnvironmentArtifact>(&mut Cursor::new(b"host environment"))
            .expect("environment")
            .content_id;
        let need = MigrationExecutionNeed::new(
            MigrationValidationTier::V0Cpu,
            ExecutionBackend::new(TEST_BACKEND).expect("backend"),
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
            &adapter,
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

        let capture = synthetic_capture(case, &adapter, &job, environment, selected);
        let mut executor =
            ScriptedExecutor::new(move |_input: &ExecutionInput<'_>| Ok(capture.clone()));
        let ExecutionCompletion::Completed {
            receipt_id,
            receipt,
        } = execute_execution_attempt(
            &mut events,
            &mut content,
            &mut executor,
            started,
            &CommandId::new(),
            ObservedAtUnixMillis::new(3),
        )
        .expect("execution completion")
        else {
            panic!("expected completed execution");
        };
        let execution = validate_collection_output_call_adapter_receipt(
            case, &adapter, &job, receipt_id, &receipt, &content,
        )
        .expect("validated receipt");
        CompletedControl {
            _directory: directory,
            content,
            adapter,
            execution,
        }
    }

    fn synthetic_capture(
        case: &AssembledCollectionF32OracleCaseInput,
        adapter: &PreparedCallAdapterInput,
        job: &PreparedCallAdapterJob,
        environment: ContentId<ExecutionEnvironmentArtifact>,
        selected: &[f32],
    ) -> ExecutionCapture {
        let mut values = vec![
            0_u8;
            usize::try_from(case.invocation().values_output().byte_length().get())
                .expect("values capacity")
        ];
        for (destination, value) in values.chunks_exact_mut(4).zip(selected) {
            destination.copy_from_slice(&value.to_bits().to_le_bytes());
        }
        let count = u32::try_from(selected.len())
            .expect("selected count")
            .to_le_bytes()
            .to_vec();
        let output_bytes = [values, count];
        let mut observed = adapter
            .request()
            .expected_outputs()
            .iter()
            .zip(&output_bytes)
            .map(|(expected, bytes)| {
                CallAdapterObservedOutputV1::from_bytes(
                    expected.argument_index(),
                    expected.buffer().clone(),
                    bytes,
                )
                .expect("observed ABI output")
            })
            .collect::<Vec<_>>();
        observed.sort_by_key(CallAdapterObservedOutputV1::argument_index);
        let result = CallAdapterResultV1::new(
            adapter.request_id(),
            adapter.request().invocation(),
            CallAdapterCompletionV1::InvokedVoid,
            observed,
        )
        .expect("adapter result");
        let result_bytes = cairn_codec::to_vec(&result).expect("result bytes");
        let captured = job
            .contract()
            .capture()
            .expected_outputs()
            .iter()
            .map(|declared| {
                let bytes = if declared.path == *adapter.request().result_path() {
                    result_bytes.clone()
                } else {
                    let position = adapter
                        .request()
                        .expected_outputs()
                        .iter()
                        .position(|expected| expected.path() == &declared.path)
                        .expect("declared ABI output");
                    output_bytes[position].clone()
                };
                CapturedOutput {
                    name: declared.name.clone(),
                    bytes,
                }
            })
            .collect::<Vec<_>>();
        let evidence = TrustedExecutionEvidence::new(
            ExecutionBackend::new(TEST_BACKEND).expect("backend"),
            environment,
            ResolvedProgramIdentity::new(adapter.request().executable().to_wire())
                .expect("program identity"),
            Vec::new(),
        )
        .expect("execution evidence");
        ExecutionCapture::new(
            ExecutionOutcome::Succeeded,
            Some(0),
            ExecutionElapsedMillis::new(1),
            Vec::new(),
            Vec::new(),
            captured,
            evidence,
        )
    }

    #[test]
    fn restricted_commit_precedes_public_local_oracle_outcome() {
        let intent = intent_outcome();
        let decision = derive_collection_output_oracle_decision(&intent).expect("decision");
        let case = assemble_collection_f32_oracle_case(
            &decision,
            &[f32_bits(1.0), f32_bits(4.0), f32_bits(3.0)],
            f32_bits(2.0),
        )
        .expect("collection case");
        let honest = complete_control(&case, b"honest implementation", &[3.0, 4.0]);
        let fault = complete_control(&case, b"missing implementation", &[3.0]);
        let prepared = admit_collection_oracle_claim(
            &intent,
            &case,
            &CollectionOracleQualificationExecution {
                adapter_input: &honest.adapter,
                execution: &honest.execution,
                content: &honest.content,
            },
            &CollectionOracleQualificationExecution {
                adapter_input: &fault.adapter,
                execution: &fault.execution,
                content: &fault.content,
            },
        )
        .expect("prepared Oracle admission");

        let restricted_state = tempfile::tempdir().expect("restricted state");
        let mut restricted = SqliteContentStore::open(
            restricted_state.path().join("content.db"),
            restricted_state.path().join("cas"),
        )
        .expect("restricted store");
        let published =
            commit_collection_oracle_admission(&mut restricted, &prepared).expect("commit");
        assert_eq!(published, prepared.public_outcome);
        assert_eq!(published.task_id(), intent.contract().task_id());
        assert_eq!(published.intent_contract(), intent.contract());
        assert_eq!(
            published.intent_contract_id().expect("contract"),
            intent.contract().identity().expect("contract")
        );
        assert_eq!(
            published.claim().identity().expect("claim"),
            prepared.claim_material().claim_id()
        );

        assert_restricted_artifacts(&restricted, &published, &prepared);
        assert_public_candidate_boundaries(&published);

        let mut failing = FailingContentStore;
        assert!(matches!(
            commit_collection_oracle_admission(&mut failing, &prepared),
            Err(IntentPromotionError::Storage(_))
        ));
    }

    fn assert_restricted_artifacts(
        restricted: &SqliteContentStore,
        published: &CollectionOracleAdmissionPublicOutcomeV1,
        prepared: &PreparedCollectionOracleAdmissionV1,
    ) {
        let mut archived = Vec::new();
        restricted
            .write_to(&published.restricted_decision(), &mut archived)
            .expect("restricted decision");
        let decoded: RestrictedCollectionOracleAdmissionDecisionV1 =
            cairn_codec::from_slice(&archived).expect("strict restricted decision");
        assert_eq!(decoded, *prepared.restricted_decision());
        archived.clear();
        restricted
            .write_to(&prepared.claim_material().receipt_id(), &mut archived)
            .expect("qualification receipt");
        let _: cairn_migration::CollectionOracleQualificationReceiptV1 =
            cairn_codec::from_slice(&archived).expect("strict qualification receipt");
    }

    fn assert_public_candidate_boundaries(published: &CollectionOracleAdmissionPublicOutcomeV1) {
        let public_bytes = cairn_codec::to_vec(published).expect("public bytes");
        let decoded: CollectionOracleAdmissionPublicOutcomeV1 =
            cairn_codec::from_slice(&public_bytes).expect("strict public outcome");
        assert_eq!(
            decoded.identity().expect("public identity"),
            published.identity().expect("id")
        );
        let public_text = String::from_utf8(public_bytes).expect("public JSON");
        for forbidden in [
            "honest_reordered",
            "missing_occurrence",
            "comparison_evidence",
            "execution_receipt",
            "limitations",
            "requalification_triggers",
        ] {
            assert!(!public_text.contains(forbidden), "leaked {forbidden}");
        }
        let candidate =
            prepare_collection_candidate_search_input(published).expect("Candidate input");
        assert_eq!(candidate.input().task_id(), published.task_id());
        assert_eq!(
            candidate.input().oracle_outcome(),
            published.identity().expect("outcome identity")
        );
        assert_eq!(
            candidate.input().oracle_claim(),
            published.claim().identity().expect("claim identity")
        );
        let candidate_text =
            String::from_utf8(candidate.bytes().to_vec()).expect("Candidate input JSON");
        for forbidden in [
            "qualification_receipt",
            "comparison",
            "execution_receipt",
            "executable",
            "expected",
        ] {
            assert!(!candidate_text.contains(forbidden), "leaked {forbidden}");
        }
        let mut invalid = serde_json::to_value(published).expect("public outcome JSON");
        invalid["schema_version"] = serde_json::json!(2);
        assert!(
            serde_json::from_value::<CollectionOracleAdmissionPublicOutcomeV1>(invalid).is_err()
        );
        let mut invalid = serde_json::to_value(published).expect("public outcome JSON");
        invalid["legacy_portfolio"] = serde_json::json!(true);
        assert!(
            serde_json::from_value::<CollectionOracleAdmissionPublicOutcomeV1>(invalid).is_err()
        );
        let mut invalid = serde_json::to_value(published).expect("public outcome JSON");
        invalid["intent_contract"]["recovery_input"] =
            serde_json::to_value(id::<IntentRecoveryInputArtifact>(b"changed recovery input"))
                .expect("recovery input JSON");
        assert!(
            serde_json::from_value::<CollectionOracleAdmissionPublicOutcomeV1>(invalid).is_err()
        );
    }

    struct FailingContentStore;

    impl ContentStore for FailingContentStore {
        fn put<T: ContentType>(
            &mut self,
            _reader: &mut dyn Read,
        ) -> Result<ContentDescriptor<T>, ContentStoreError> {
            Err(ContentStoreError::Metadata {
                message: "injected restricted commit failure".to_owned(),
            })
        }

        fn write_to<T: ContentType>(
            &self,
            content_id: &ContentId<T>,
            _writer: &mut dyn Write,
        ) -> Result<ContentDescriptor<T>, ContentStoreError> {
            Err(ContentStoreError::NotFound {
                content_id: content_id.to_wire(),
            })
        }
    }
}
