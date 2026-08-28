//! Deterministic Intent Admission types, promotion gate, and first contract-only Oracle consumer.

use std::{fmt, str::FromStr};

use cairn_migration::{
    IntentHypothesisSetProposalV1, IntentRecoveryInputArtifact, IntentRecoveryInputV1,
    SirCallerClaimId, SirHypothesisId, SirIntentHypothesisSetProposalArtifact,
    UserIntentDecisionRequestArtifact, UserIntentDecisionRequestV1,
    derive_user_intent_decision_requests,
};
use cairn_protocol::{ContentId, ContentType, SchemaVersion, TaskId};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

const MAX_AUTHORITY_SUBJECT_BYTES: usize = 128;
const MAX_COLLECTION_ELEMENTS: usize = 1_048_576;
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

/// First immutable admitted migration-intent contract.
pub enum MigrationIntentContractArtifact {}

impl ContentType for MigrationIntentContractArtifact {
    const DOMAIN: &'static str = "migration.intent-contract.v1";
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

/// Exact semantic identity of one collection element used by the first Oracle comparator.
pub enum CollectionOracleElementArtifact {}

impl ContentType for CollectionOracleElementArtifact {
    const DOMAIN: &'static str = "migration.oracle-collection-element.v1";
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

/// Count reported by a collection-producing implementation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CollectionReportedCount(u32);

impl CollectionReportedCount {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Trusted expected collection produced by the selected Oracle reference.
///
/// A candidate observation cannot substitute for trusted expected values.
///
/// ```compile_fail
/// use cairn_admission::{
///     CollectionReportedCount, ExpectedCollectionOracleOutputV1,
///     ObservedCollectionOracleOutputV1,
/// };
/// fn require_expected(_: ExpectedCollectionOracleOutputV1) {}
/// let observed = ObservedCollectionOracleOutputV1::new(
///     Vec::new(),
///     CollectionReportedCount::new(0),
/// ).unwrap();
/// require_expected(observed);
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExpectedCollectionOracleOutputV1 {
    elements: Vec<ContentId<CollectionOracleElementArtifact>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedCollectionOracleOutputWire {
    elements: Vec<ContentId<CollectionOracleElementArtifact>>,
}

impl ExpectedCollectionOracleOutputV1 {
    /// Creates a bounded trusted expected collection.
    ///
    /// # Errors
    ///
    /// Rejects expected collections exceeding the current-V1 element bound.
    pub fn new(
        elements: Vec<ContentId<CollectionOracleElementArtifact>>,
    ) -> Result<Self, IntentPromotionError> {
        validate_collection_bound(&elements)?;
        Ok(Self { elements })
    }
}

impl TryFrom<ExpectedCollectionOracleOutputWire> for ExpectedCollectionOracleOutputV1 {
    type Error = IntentPromotionError;

    fn try_from(wire: ExpectedCollectionOracleOutputWire) -> Result<Self, Self::Error> {
        Self::new(wire.elements)
    }
}

impl<'de> Deserialize<'de> for ExpectedCollectionOracleOutputV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        ExpectedCollectionOracleOutputWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Candidate observation whose independently reported count may be wrong.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ObservedCollectionOracleOutputV1 {
    elements: Vec<ContentId<CollectionOracleElementArtifact>>,
    reported_count: CollectionReportedCount,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservedCollectionOracleOutputWire {
    elements: Vec<ContentId<CollectionOracleElementArtifact>>,
    reported_count: CollectionReportedCount,
}

impl ObservedCollectionOracleOutputV1 {
    /// Creates a bounded candidate observation without trusting its reported count.
    ///
    /// # Errors
    ///
    /// Rejects observations exceeding the current-V1 element bound.
    pub fn new(
        elements: Vec<ContentId<CollectionOracleElementArtifact>>,
        reported_count: CollectionReportedCount,
    ) -> Result<Self, IntentPromotionError> {
        validate_collection_bound(&elements)?;
        Ok(Self {
            elements,
            reported_count,
        })
    }
}

impl TryFrom<ObservedCollectionOracleOutputWire> for ObservedCollectionOracleOutputV1 {
    type Error = IntentPromotionError;

    fn try_from(wire: ObservedCollectionOracleOutputWire) -> Result<Self, Self::Error> {
        Self::new(wire.elements, wire.reported_count)
    }
}

impl<'de> Deserialize<'de> for ObservedCollectionOracleOutputV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        ObservedCollectionOracleOutputWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Concrete comparator decision selected from an admitted contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionOutputOraclePolicyV1 {
    ExactMultisetAndCount,
    ExactSequenceAndCount,
}

/// Explicit comparison result; no stored pass boolean erases the failure class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionOutputComparisonV1 {
    Equivalent,
    ReportedCountMismatch,
    ElementMultisetMismatch,
    ElementSequenceMismatch,
}

/// Contract-bound Oracle decision.
pub struct CollectionOutputOracleDecisionV1 {
    contract: ContentId<MigrationIntentContractArtifact>,
    selection_claim: SirCallerClaimId,
    policy: CollectionOutputOraclePolicyV1,
}

impl CollectionOutputOracleDecisionV1 {
    #[must_use]
    pub const fn policy(&self) -> CollectionOutputOraclePolicyV1 {
        self.policy
    }

    #[must_use]
    pub const fn contract(&self) -> ContentId<MigrationIntentContractArtifact> {
        self.contract
    }

    #[must_use]
    pub const fn selection_claim(&self) -> &SirCallerClaimId {
        &self.selection_claim
    }

    #[must_use]
    pub fn compare(
        &self,
        expected: &ExpectedCollectionOracleOutputV1,
        actual: &ObservedCollectionOracleOutputV1,
    ) -> CollectionOutputComparisonV1 {
        if u32::try_from(expected.elements.len()).unwrap_or(u32::MAX) != actual.reported_count.get()
        {
            return CollectionOutputComparisonV1::ReportedCountMismatch;
        }
        match self.policy {
            CollectionOutputOraclePolicyV1::ExactSequenceAndCount => {
                if expected.elements == actual.elements {
                    CollectionOutputComparisonV1::Equivalent
                } else {
                    CollectionOutputComparisonV1::ElementSequenceMismatch
                }
            }
            CollectionOutputOraclePolicyV1::ExactMultisetAndCount => {
                let mut expected_elements = expected.elements.clone();
                let mut actual_elements = actual.elements.clone();
                expected_elements.sort_by_key(ContentId::to_wire);
                actual_elements.sort_by_key(ContentId::to_wire);
                if expected_elements == actual_elements {
                    CollectionOutputComparisonV1::Equivalent
                } else {
                    CollectionOutputComparisonV1::ElementMultisetMismatch
                }
            }
        }
    }
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
    Ok(CollectionOutputOracleDecisionV1 {
        contract: outcome.contract.identity()?,
        selection_claim: contract.selection_claim.clone(),
        policy,
    })
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

fn migration_error(error: impl fmt::Display) -> IntentPromotionError {
    IntentPromotionError::Migration(error.to_string())
}

fn validate_collection_bound<T>(values: &[T]) -> Result<(), IntentPromotionError> {
    if values.len() > MAX_COLLECTION_ELEMENTS {
        return Err(IntentPromotionError::InvalidStructure(
            "collection Oracle element count",
        ));
    }
    Ok(())
}
