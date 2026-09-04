//! Deterministic, task-generic Intent Admission types and promotion gate.

use std::{fmt, str::FromStr};

use crate::{
    AuthoritativeIntentClaimV1, IntentDecisionRequestBatchV1, IntentHypothesisSetProposalV1,
    IntentRecoveryInputArtifact, IntentRecoveryInputV1, MigrationIntentContractArtifact,
    SirCallerClaimId, SirHypothesisId, SirIntentHypothesisSetProposalArtifact,
    UserIntentDecisionRequestArtifact, UserIntentDecisionRequestV1,
    derive_user_intent_decision_requests,
};
use cairn_protocol::{ContentId, ContentType, SchemaVersion, TaskId};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

const MAX_AUTHORITY_SUBJECT_BYTES: usize = 128;
const INTENT_USER_DECISION_GATE_V1: &[u8] = include_bytes!("intent_promotion.rs");

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
/// use cairn_migration::{UserIntentDecisionArtifact, UserIntentDecisionRequestArtifact};
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

/// Public outcome returned only after product composition commits restricted state.
pub enum IntentAdmissionPublicOutcomeArtifact {}

impl ContentType for IntentAdmissionPublicOutcomeArtifact {
    const DOMAIN: &'static str = "migration.intent-admission-public-outcome.v1";
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UserIntentAuthorityScopeV1 {
    claims: Vec<SirCallerClaimId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UserIntentAuthorityScopeWire {
    claims: Vec<SirCallerClaimId>,
}

impl UserIntentAuthorityScopeV1 {
    /// Creates a bounded, sorted set of caller-authoritative claims.
    ///
    /// # Errors
    ///
    /// Rejects an empty, oversized, unsorted, or duplicate claim set.
    pub fn new(claims: Vec<SirCallerClaimId>) -> Result<Self, IntentPromotionError> {
        if claims.is_empty()
            || claims.len() > 16
            || claims.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(IntentPromotionError::InvalidStructure(
                "user intent authority claim order",
            ));
        }
        Ok(Self { claims })
    }

    #[must_use]
    pub fn claims(&self) -> &[SirCallerClaimId] {
        &self.claims
    }
}

impl TryFrom<UserIntentAuthorityScopeWire> for UserIntentAuthorityScopeV1 {
    type Error = IntentPromotionError;

    fn try_from(wire: UserIntentAuthorityScopeWire) -> Result<Self, Self::Error> {
        Self::new(wire.claims)
    }
}

impl<'de> Deserialize<'de> for UserIntentAuthorityScopeV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        UserIntentAuthorityScopeWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
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

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn scope(&self) -> &UserIntentAuthorityScopeV1 {
        &self.scope
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

/// Exact response to one scoped user-decision request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum UserIntentDecisionResponseV1 {
    SelectHypothesis { hypothesis: SirHypothesisId },
    KeepUnknown,
    ProvideAuthoritativeClaim { claim: UserProvidedIntentClaimV1 },
}

/// Task-authority semantics supplied when none of the proposed hypotheses is acceptable.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UserProvidedIntentClaimV1 {
    layer: crate::SirIntentLayer,
    semantics: crate::SirHypothesisClaim,
    domain: crate::SirIntentDomain,
}

impl UserProvidedIntentClaimV1 {
    #[must_use]
    pub const fn new(
        layer: crate::SirIntentLayer,
        semantics: crate::SirHypothesisClaim,
        domain: crate::SirIntentDomain,
    ) -> Self {
        Self {
            layer,
            semantics,
            domain,
        }
    }

    #[must_use]
    pub const fn layer(&self) -> crate::SirIntentLayer {
        self.layer
    }

    #[must_use]
    pub const fn semantics(&self) -> &crate::SirHypothesisClaim {
        &self.semantics
    }

    #[must_use]
    pub const fn domain(&self) -> &crate::SirIntentDomain {
        &self.domain
    }
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

    #[must_use]
    pub const fn response(&self) -> &UserIntentDecisionResponseV1 {
        &self.response
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

/// One mechanically admitted answer in an exact administrator decision set.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmittedIntentDecisionV1 {
    request: ContentId<UserIntentDecisionRequestArtifact>,
    authority_grant: ContentId<UserIntentAuthorityGrantArtifact>,
    user_decision: ContentId<UserIntentDecisionArtifact>,
    selected_hypothesis: Option<SirHypothesisId>,
    admitted_claim: AuthoritativeIntentClaimV1,
}

impl AdmittedIntentDecisionV1 {
    #[must_use]
    pub const fn request(&self) -> ContentId<UserIntentDecisionRequestArtifact> {
        self.request
    }

    #[must_use]
    pub const fn admitted_claim(&self) -> &AuthoritativeIntentClaimV1 {
        &self.admitted_claim
    }
}

/// Immutable first admitted intent contract.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MigrationIntentContractV1 {
    schema_version: SchemaVersion,
    task_id: TaskId,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
    decision_requests: ContentId<crate::IntentDecisionRequestBatchArtifact>,
    decisions: Vec<AdmittedIntentDecisionV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationIntentContractWire {
    schema_version: SchemaVersion,
    task_id: TaskId,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
    decision_requests: ContentId<crate::IntentDecisionRequestBatchArtifact>,
    decisions: Vec<AdmittedIntentDecisionV1>,
}

impl MigrationIntentContractV1 {
    fn validate(&self) -> Result<(), IntentPromotionError> {
        if self.schema_version != schema_v1() {
            return Err(IntentPromotionError::InvalidStructure(
                "migration intent contract schema",
            ));
        }
        let unique_requests = self
            .decisions
            .iter()
            .map(|decision| decision.request().to_wire())
            .collect::<std::collections::BTreeSet<_>>();
        if self.decisions.is_empty() || unique_requests.len() != self.decisions.len() {
            return Err(IntentPromotionError::InvalidStructure(
                "admitted intent decision order",
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

    #[must_use]
    pub const fn proposal(&self) -> ContentId<SirIntentHypothesisSetProposalArtifact> {
        self.proposal
    }

    #[must_use]
    pub const fn decision_requests(&self) -> ContentId<crate::IntentDecisionRequestBatchArtifact> {
        self.decision_requests
    }

    #[must_use]
    pub fn decisions(&self) -> &[AdmittedIntentDecisionV1] {
        &self.decisions
    }

    #[must_use]
    pub fn admitted_claims(&self) -> impl ExactSizeIterator<Item = &AuthoritativeIntentClaimV1> {
        self.decisions
            .iter()
            .map(AdmittedIntentDecisionV1::admitted_claim)
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
            decision_requests: wire.decision_requests,
            decisions: wire.decisions,
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
    user_decisions: Vec<ContentId<UserIntentDecisionArtifact>>,
    contract: ContentId<MigrationIntentContractArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestrictedIntentAdmissionDecisionWire {
    schema_version: SchemaVersion,
    mechanism: ContentId<IntentUserDecisionGateArtifact>,
    user_decisions: Vec<ContentId<UserIntentDecisionArtifact>>,
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
        let unique_decisions = self
            .user_decisions
            .iter()
            .map(ContentId::to_wire)
            .collect::<std::collections::BTreeSet<_>>();
        if self.schema_version != schema_v1()
            || self.mechanism != intent_user_decision_gate_id()?
            || self.user_decisions.is_empty()
            || unique_decisions.len() != self.user_decisions.len()
        {
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
            user_decisions: wire.user_decisions,
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

    /// Derives the exact public outcome identity.
    ///
    /// # Errors
    ///
    /// Rejects invalid current-V1 structure or typed identity encoding failure.
    pub fn identity(
        &self,
    ) -> Result<ContentId<IntentAdmissionPublicOutcomeArtifact>, IntentPromotionError> {
        self.validate()?;
        derive_id(self)
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

/// Prepared deterministic gate result; product composition must archive the restricted decision
/// before publishing `public_outcome`.
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

/// One exact request, grant, and answer offered to the mechanical promotion gate.
pub struct IntentDecisionMaterialV1<'a> {
    pub request: &'a UserIntentDecisionRequestV1,
    pub grant: &'a UserIntentAuthorityGrantV1,
    pub decision: &'a UserIntentDecisionV1,
}

/// Mechanically promotes one complete task-authority decision set.
///
/// # Errors
///
/// Fails closed on any identity, task, request, option, authority-scope, or caller-claim mismatch.
#[allow(
    clippy::too_many_lines,
    reason = "one linear mechanical gate keeps the complete administrator decision-set binding visible"
)]
pub fn promote_user_intent(
    proposal_id: ContentId<SirIntentHypothesisSetProposalArtifact>,
    proposal: &IntentHypothesisSetProposalV1,
    recovery_input_id: ContentId<IntentRecoveryInputArtifact>,
    recovery_input: &IntentRecoveryInputV1,
    requests: &IntentDecisionRequestBatchV1,
    decisions: &[IntentDecisionMaterialV1<'_>],
) -> Result<PreparedIntentAdmissionV1, IntentPromotionError> {
    let derived = derive_user_intent_decision_requests(
        proposal_id,
        proposal,
        recovery_input_id,
        recovery_input,
    )
    .map_err(migration_error)?;
    if &derived != requests || decisions.len() != requests.requests().len() {
        return Err(IntentPromotionError::Binding(
            "complete decision request batch",
        ));
    }
    let mut admitted = Vec::with_capacity(decisions.len());
    for (expected, material) in requests.requests().iter().zip(decisions) {
        if material.request != expected {
            return Err(IntentPromotionError::Binding("decision request order"));
        }
        let request_id = material.request.identity().map_err(migration_error)?;
        let grant_id = material.grant.identity()?;
        let decision_id = material.decision.identity()?;
        if material.decision.request != request_id
            || material.decision.authority_grant != grant_id
            || material.grant.task_id != recovery_input.task_id()
            || material.request.proposal() != proposal_id
            || material.request.recovery_input() != recovery_input_id
        {
            return Err(IntentPromotionError::Binding("decision input"));
        }
        let (selected_hypothesis, admitted_claim) = match &material.decision.response {
            UserIntentDecisionResponseV1::SelectHypothesis { hypothesis } => {
                if !material
                    .request
                    .options()
                    .iter()
                    .any(|option| option.hypothesis() == hypothesis)
                {
                    return Err(IntentPromotionError::UnofferedHypothesis);
                }
                let selected = proposal
                    .submission()
                    .hypotheses()
                    .iter()
                    .find(|candidate| candidate.id() == hypothesis)
                    .ok_or(IntentPromotionError::UnofferedHypothesis)?;
                let operation = crate::OperationIntentV1::new(
                    scoped_caller_claims(material.grant, recovery_input)?,
                    selected.layer(),
                    selected.claim().clone(),
                    selected.domain().clone(),
                )
                .map_err(|error| IntentPromotionError::Migration(error.to_string()))?;
                (
                    Some(hypothesis.clone()),
                    AuthoritativeIntentClaimV1::new(operation),
                )
            }
            UserIntentDecisionResponseV1::ProvideAuthoritativeClaim { claim } => {
                let operation = crate::OperationIntentV1::new(
                    scoped_caller_claims(material.grant, recovery_input)?,
                    claim.layer(),
                    claim.semantics().clone(),
                    claim.domain().clone(),
                )
                .map_err(|error| IntentPromotionError::Migration(error.to_string()))?;
                (None, AuthoritativeIntentClaimV1::new(operation))
            }
            UserIntentDecisionResponseV1::KeepUnknown => {
                return Err(IntentPromotionError::KeptUnknown);
            }
        };
        validate_authority_scope(material.grant, recovery_input, &admitted_claim)?;
        admitted.push(AdmittedIntentDecisionV1 {
            request: request_id,
            authority_grant: grant_id,
            user_decision: decision_id,
            selected_hypothesis,
            admitted_claim,
        });
    }

    let contract = MigrationIntentContractV1 {
        schema_version: schema_v1(),
        task_id: recovery_input.task_id(),
        recovery_input: recovery_input_id,
        proposal: proposal_id,
        decision_requests: requests.identity().map_err(migration_error)?,
        decisions: admitted,
    };
    contract.validate()?;
    let restricted_decision = RestrictedIntentAdmissionDecisionV1 {
        schema_version: schema_v1(),
        mechanism: intent_user_decision_gate_id()?,
        user_decisions: contract
            .decisions()
            .iter()
            .map(|decision| decision.user_decision)
            .collect(),
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

/// Resolves one authority scope against the claims a task's caller actually declared.
///
/// An administrator decision is scoped by naming caller claims. Two sides read that scope: the App
/// API when it records the decision, and Intent Admission when it promotes one. They must decide
/// the same way, so both call this rather than each interpreting the field, and a scope naming a
/// claim the caller never declared is refused where the administrator can still correct it.
///
/// # Errors
///
/// Names the first claim in the scope that the caller declaration does not carry.
pub fn resolve_authority_scope_claims(
    scope: &UserIntentAuthorityScopeV1,
    caller_claims: &[crate::SirCallerClaimV1],
) -> Result<Vec<crate::SirCallerClaimV1>, IntentPromotionError> {
    scope
        .claims()
        .iter()
        .map(|authority_claim| {
            caller_claims
                .iter()
                .find(|claim| claim.id() == authority_claim)
                .cloned()
                .ok_or_else(|| {
                    IntentPromotionError::UndeclaredAuthorityClaim(authority_claim.clone())
                })
        })
        .collect()
}

fn scoped_caller_claims(
    grant: &UserIntentAuthorityGrantV1,
    recovery_input: &IntentRecoveryInputV1,
) -> Result<Vec<crate::SirCallerClaimV1>, IntentPromotionError> {
    resolve_authority_scope_claims(grant.scope(), recovery_input.request().caller().claims())
}

fn validate_authority_scope(
    grant: &UserIntentAuthorityGrantV1,
    recovery_input: &IntentRecoveryInputV1,
    admitted_claim: &AuthoritativeIntentClaimV1,
) -> Result<(), IntentPromotionError> {
    let declared =
        resolve_authority_scope_claims(grant.scope(), recovery_input.request().caller().claims())?;
    let admitted_caller_claims = admitted_claim.operation().caller_claims();
    if declared.len() != admitted_caller_claims.len()
        || !declared
            .iter()
            .zip(admitted_caller_claims)
            .all(|(declared, admitted)| declared.id() == admitted.id())
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
    #[error("the authority scope names a claim the caller did not declare: {}", .0.as_str())]
    UndeclaredAuthorityClaim(crate::SirCallerClaimId),
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

fn migration_error(error: impl fmt::Display) -> IntentPromotionError {
    IntentPromotionError::Migration(error.to_string())
}
