//! Model-free, proposal-preserving Intent Admission triage.

use std::collections::{BTreeMap, BTreeSet};

use cairn_protocol::{ContentId, ContentType};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    IntentHypothesisSetProposalV1, IntentRecoveryInputArtifact, IntentRecoveryInputV1,
    SirConflictId, SirDeclaredUnknownId, SirDeclaredUnknownKind, SirDeclaredUnknownQuestion,
    SirDisambiguationTargetV1, SirHypothesisClaim, SirHypothesisId, SirIntentClaimRefV1,
    SirIntentDomain, SirIntentHypothesisSetProposalArtifact, SirIntentLayer, SirUnknownId,
    SirUnknownKind, SirUnknownQuestion,
};

const SCHEMA_V1: u16 = 1;
const MAX_DECISION_REQUESTS: usize = 32;
const MAX_DECISION_OPTIONS: usize = 16;
const MAX_CONFLICTS: usize = 16;
const MAX_CALLER_CONTEXT: usize = 32;

/// Content domain for one exact user-intent decision request.
///
/// Request identities cannot be substituted for proposal identities.
///
/// ```compile_fail
/// use cairn_migration::{
///     SirIntentHypothesisSetProposalArtifact, UserIntentDecisionRequestArtifact,
/// };
/// use cairn_protocol::ContentId;
/// fn require_decision_request(_: ContentId<UserIntentDecisionRequestArtifact>) {}
/// let proposal = ContentId::<SirIntentHypothesisSetProposalArtifact>::derive(b"proposal").unwrap();
/// require_decision_request(proposal);
/// ```
///
/// Conflict and unknown identities also remain statically distinct.
///
/// ```compile_fail
/// use cairn_migration::{SirConflictId, SirUnknownId};
/// fn require_unknown(_: SirUnknownId) {}
/// let conflict = SirConflictId::new("output-order-conflict").unwrap();
/// require_unknown(conflict);
/// ```
///
/// Hypothesis and conflict identities cannot be interchanged either.
///
/// ```compile_fail
/// use cairn_migration::{SirConflictId, SirHypothesisId};
/// fn require_conflict(_: SirConflictId) {}
/// let hypothesis = SirHypothesisId::new("order-unspecified").unwrap();
/// require_conflict(hypothesis);
/// ```
pub enum UserIntentDecisionRequestArtifact {}

impl ContentType for UserIntentDecisionRequestArtifact {
    const DOMAIN: &'static str = "migration.user-intent-decision-request.v1";
}

/// Content domain for a canonical batch emitted by the DEV-007 triage process.
pub enum IntentDecisionRequestBatchArtifact {}

impl ContentType for IntentDecisionRequestBatchArtifact {
    const DOMAIN: &'static str = "migration.intent-decision-request-batch.v1";
}

/// The three semantically distinct ways an actual task authority may answer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UserIntentDecisionResponseKind {
    SelectHypothesis,
    KeepUnknown,
    ProvideAuthoritativeClaim,
}

const PERMITTED_RESPONSES: [UserIntentDecisionResponseKind; 3] = [
    UserIntentDecisionResponseKind::SelectHypothesis,
    UserIntentDecisionResponseKind::KeepUnknown,
    UserIntentDecisionResponseKind::ProvideAuthoritativeClaim,
];

/// Caller-owned unknown retained as context without asserting that SIR resolved its identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UserIntentCallerUnknownContextV1 {
    id: SirDeclaredUnknownId,
    kind: SirDeclaredUnknownKind,
    question: SirDeclaredUnknownQuestion,
}

/// One exact, still-untrusted SIR hypothesis offered to the actual task authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UserIntentDecisionOptionV1 {
    hypothesis: SirHypothesisId,
    layer: SirIntentLayer,
    claim: SirHypothesisClaim,
    domain: SirIntentDomain,
}

impl UserIntentDecisionOptionV1 {
    #[must_use]
    pub const fn hypothesis(&self) -> &SirHypothesisId {
        &self.hypothesis
    }

    #[must_use]
    pub const fn claim(&self) -> &SirHypothesisClaim {
        &self.claim
    }
}

/// One claim-scoped question that evidence cannot decide on behalf of the user.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UserIntentDecisionRequestV1 {
    schema_version: u16,
    proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    unknown: SirUnknownId,
    question: SirUnknownQuestion,
    caller_unknown_context: Vec<UserIntentCallerUnknownContextV1>,
    conflicts: Vec<SirConflictId>,
    options: Vec<UserIntentDecisionOptionV1>,
    permitted_responses: [UserIntentDecisionResponseKind; 3],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UserIntentDecisionRequestWire {
    schema_version: u16,
    proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    unknown: SirUnknownId,
    question: SirUnknownQuestion,
    caller_unknown_context: Vec<UserIntentCallerUnknownContextV1>,
    conflicts: Vec<SirConflictId>,
    options: Vec<UserIntentDecisionOptionV1>,
    permitted_responses: [UserIntentDecisionResponseKind; 3],
}

impl UserIntentDecisionRequestV1 {
    fn new(
        proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
        recovery_input: ContentId<IntentRecoveryInputArtifact>,
        unknown: SirUnknownId,
        question: SirUnknownQuestion,
        caller_unknown_context: Vec<UserIntentCallerUnknownContextV1>,
        conflicts: Vec<SirConflictId>,
        options: Vec<UserIntentDecisionOptionV1>,
    ) -> Result<Self, IntentAdmissionError> {
        validate_request_parts(&caller_unknown_context, &conflicts, &options)?;
        Ok(Self {
            schema_version: SCHEMA_V1,
            proposal,
            recovery_input,
            unknown,
            question,
            caller_unknown_context,
            conflicts,
            options,
            permitted_responses: PERMITTED_RESPONSES,
        })
    }

    #[must_use]
    pub const fn unknown(&self) -> &SirUnknownId {
        &self.unknown
    }

    #[must_use]
    pub const fn proposal(&self) -> ContentId<SirIntentHypothesisSetProposalArtifact> {
        self.proposal
    }

    #[must_use]
    pub const fn recovery_input(&self) -> ContentId<IntentRecoveryInputArtifact> {
        self.recovery_input
    }

    #[must_use]
    pub fn options(&self) -> &[UserIntentDecisionOptionV1] {
        &self.options
    }

    /// Derives the semantic content identity of this exact request.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or typed identity derivation fails.
    pub fn identity(
        &self,
    ) -> Result<ContentId<UserIntentDecisionRequestArtifact>, IntentAdmissionError> {
        let bytes = cairn_codec::to_vec(self)?;
        ContentId::derive(&bytes).map_err(|error| IntentAdmissionError::Codec(error.to_string()))
    }
}

impl TryFrom<UserIntentDecisionRequestWire> for UserIntentDecisionRequestV1 {
    type Error = IntentAdmissionError;

    fn try_from(wire: UserIntentDecisionRequestWire) -> Result<Self, Self::Error> {
        if wire.schema_version != SCHEMA_V1 || wire.permitted_responses != PERMITTED_RESPONSES {
            return Err(IntentAdmissionError::InvalidStructure(
                "user decision request V1 envelope",
            ));
        }
        Self::new(
            wire.proposal,
            wire.recovery_input,
            wire.unknown,
            wire.question,
            wire.caller_unknown_context,
            wire.conflicts,
            wire.options,
        )
    }
}

impl<'de> Deserialize<'de> for UserIntentDecisionRequestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        UserIntentDecisionRequestWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Canonical output from one model-free public triage invocation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentDecisionRequestBatchV1 {
    schema_version: u16,
    proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    requests: Vec<UserIntentDecisionRequestV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentDecisionRequestBatchWire {
    schema_version: u16,
    proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
    requests: Vec<UserIntentDecisionRequestV1>,
}

impl IntentDecisionRequestBatchV1 {
    fn new(
        proposal: ContentId<SirIntentHypothesisSetProposalArtifact>,
        recovery_input: ContentId<IntentRecoveryInputArtifact>,
        requests: Vec<UserIntentDecisionRequestV1>,
    ) -> Result<Self, IntentAdmissionError> {
        if requests.is_empty() || requests.len() > MAX_DECISION_REQUESTS {
            return Err(IntentAdmissionError::InvalidStructure(
                "decision request count",
            ));
        }
        validate_strict_order(requests.iter().map(|request| request.unknown.as_str()))?;
        if requests
            .iter()
            .any(|request| request.proposal != proposal || request.recovery_input != recovery_input)
        {
            return Err(IntentAdmissionError::InvalidStructure(
                "decision request batch binding",
            ));
        }
        Ok(Self {
            schema_version: SCHEMA_V1,
            proposal,
            recovery_input,
            requests,
        })
    }

    #[must_use]
    pub fn requests(&self) -> &[UserIntentDecisionRequestV1] {
        &self.requests
    }

    #[must_use]
    pub const fn proposal(&self) -> ContentId<SirIntentHypothesisSetProposalArtifact> {
        self.proposal
    }

    #[must_use]
    pub const fn recovery_input(&self) -> ContentId<IntentRecoveryInputArtifact> {
        self.recovery_input
    }

    /// Derives the semantic content identity of the exact batch.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or typed identity derivation fails.
    pub fn identity(
        &self,
    ) -> Result<ContentId<IntentDecisionRequestBatchArtifact>, IntentAdmissionError> {
        let bytes = cairn_codec::to_vec(self)?;
        ContentId::derive(&bytes).map_err(|error| IntentAdmissionError::Codec(error.to_string()))
    }
}

impl TryFrom<IntentDecisionRequestBatchWire> for IntentDecisionRequestBatchV1 {
    type Error = IntentAdmissionError;

    fn try_from(wire: IntentDecisionRequestBatchWire) -> Result<Self, Self::Error> {
        if wire.schema_version != SCHEMA_V1 {
            return Err(IntentAdmissionError::InvalidStructure(
                "decision request batch schema",
            ));
        }
        Self::new(wire.proposal, wire.recovery_input, wire.requests)
    }
}

impl<'de> Deserialize<'de> for IntentDecisionRequestBatchV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentDecisionRequestBatchWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Fail-closed errors from deterministic public Intent triage.
#[derive(Debug, Error)]
pub enum IntentAdmissionError {
    #[error("SIR contract rejected: {0}")]
    Sir(#[from] crate::SirError),
    #[error("canonical codec rejected the admission material: {0}")]
    Codec(String),
    #[error("{0} identity does not match its canonical bytes")]
    IdentityMismatch(&'static str),
    #[error("proposal has no desired-semantics unknown")]
    NoDesiredSemanticsUnknown,
    #[error("desired-semantics unknown {unknown} lacks a closed conflict/option graph")]
    IncompleteDecisionClosure { unknown: String },
    #[error("invalid Intent Admission structure: {0}")]
    InvalidStructure(&'static str),
}

impl From<cairn_codec::CodecError> for IntentAdmissionError {
    fn from(error: cairn_codec::CodecError) -> Self {
        Self::Codec(error.to_string())
    }
}

/// Mechanically derives user-decision requests from an exact proposal/input pair.
///
/// This function does not admit a hypothesis. It merely preserves a closed set of competing
/// proposal options for an actual task authority.
///
/// # Errors
///
/// Fails on identity/binding mismatch, invalid caller references, absence of desired-semantics
/// unknowns, or any desired-semantics unknown without a common experiment/conflict closure.
pub fn derive_user_intent_decision_requests(
    proposal_id: ContentId<SirIntentHypothesisSetProposalArtifact>,
    proposal: &IntentHypothesisSetProposalV1,
    recovery_input_id: ContentId<IntentRecoveryInputArtifact>,
    recovery_input: &IntentRecoveryInputV1,
) -> Result<IntentDecisionRequestBatchV1, IntentAdmissionError> {
    if proposal.identity()? != proposal_id {
        return Err(IntentAdmissionError::IdentityMismatch("proposal"));
    }
    if recovery_input.identity()? != recovery_input_id {
        return Err(IntentAdmissionError::IdentityMismatch("recovery input"));
    }
    if proposal.recovery_input() != recovery_input_id {
        return Err(IntentAdmissionError::InvalidStructure(
            "proposal recovery-input binding",
        ));
    }
    proposal
        .submission()
        .validate_against_recovery_input(recovery_input)?;

    let submission = proposal.submission();
    let hypothesis_by_id = submission
        .hypotheses()
        .iter()
        .map(|hypothesis| (hypothesis.id().as_str(), hypothesis))
        .collect::<BTreeMap<_, _>>();
    let conflicts = submission
        .conflicts()
        .iter()
        .map(|conflict| (conflict.id().as_str(), conflict))
        .collect::<BTreeMap<_, _>>();
    let caller_unknown_context = recovery_input
        .request()
        .caller()
        .unknowns()
        .iter()
        .map(|unknown| UserIntentCallerUnknownContextV1 {
            id: unknown.id().clone(),
            kind: unknown.kind(),
            question: unknown.question().clone(),
        })
        .collect::<Vec<_>>();

    let desired_unknowns = submission
        .unknowns()
        .iter()
        .filter(|unknown| unknown.kind() == SirUnknownKind::DesiredSemantics)
        .collect::<Vec<_>>();
    if desired_unknowns.is_empty() {
        return Err(IntentAdmissionError::NoDesiredSemanticsUnknown);
    }

    let mut requests = Vec::with_capacity(desired_unknowns.len());
    for unknown in desired_unknowns {
        let conflict_ids = conflicts_for_unknown(submission, unknown.id());
        let mut option_ids = BTreeSet::new();
        for conflict_id in &conflict_ids {
            let Some(conflict) = conflicts.get(conflict_id.as_str()) else {
                return Err(IntentAdmissionError::IncompleteDecisionClosure {
                    unknown: unknown.id().as_str().to_owned(),
                });
            };
            for claim in conflict.claims() {
                if let SirIntentClaimRefV1::Hypothesis { hypothesis } = claim {
                    option_ids.insert(hypothesis.as_str());
                }
            }
        }
        if conflict_ids.is_empty() || option_ids.len() < 2 {
            return Err(IntentAdmissionError::IncompleteDecisionClosure {
                unknown: unknown.id().as_str().to_owned(),
            });
        }
        let options = option_ids
            .into_iter()
            .map(|hypothesis_id| {
                let option = hypothesis_by_id.get(hypothesis_id).ok_or_else(|| {
                    IntentAdmissionError::IncompleteDecisionClosure {
                        unknown: unknown.id().as_str().to_owned(),
                    }
                })?;
                Ok(UserIntentDecisionOptionV1 {
                    hypothesis: option.id().clone(),
                    layer: option.layer(),
                    claim: option.claim().clone(),
                    domain: option.domain().clone(),
                })
            })
            .collect::<Result<Vec<_>, IntentAdmissionError>>()?;
        requests.push(UserIntentDecisionRequestV1::new(
            proposal_id,
            recovery_input_id,
            unknown.id().clone(),
            unknown.question().clone(),
            caller_unknown_context.clone(),
            conflict_ids,
            options,
        )?);
    }
    IntentDecisionRequestBatchV1::new(proposal_id, recovery_input_id, requests)
}

fn conflicts_for_unknown(
    submission: &crate::SirProposalSubmissionV1,
    unknown: &SirUnknownId,
) -> Vec<SirConflictId> {
    let mut conflict_ids = BTreeSet::new();
    for experiment in submission.disambiguation_experiments() {
        let targets_unknown = experiment.targets().iter().any(
            |target| matches!(target, SirDisambiguationTargetV1::Unknown { unknown: target } if target == unknown),
        );
        if targets_unknown {
            for target in experiment.targets() {
                if let SirDisambiguationTargetV1::Conflict { conflict } = target {
                    conflict_ids.insert(conflict.clone());
                }
            }
        }
    }
    conflict_ids.into_iter().collect()
}

fn validate_request_parts(
    caller_unknown_context: &[UserIntentCallerUnknownContextV1],
    conflicts: &[SirConflictId],
    options: &[UserIntentDecisionOptionV1],
) -> Result<(), IntentAdmissionError> {
    if caller_unknown_context.len() > MAX_CALLER_CONTEXT
        || conflicts.is_empty()
        || conflicts.len() > MAX_CONFLICTS
        || options.len() < 2
        || options.len() > MAX_DECISION_OPTIONS
    {
        return Err(IntentAdmissionError::InvalidStructure(
            "user decision request bounds",
        ));
    }
    validate_strict_order(caller_unknown_context.iter().map(|value| value.id.as_str()))?;
    validate_strict_order(conflicts.iter().map(SirConflictId::as_str))?;
    validate_strict_order(options.iter().map(|value| value.hypothesis.as_str()))
}

fn validate_strict_order<'a>(
    values: impl Iterator<Item = &'a str>,
) -> Result<(), IntentAdmissionError> {
    let mut previous = None;
    for value in values {
        if previous.is_some_and(|prior| prior >= value) {
            return Err(IntentAdmissionError::InvalidStructure(
                "collection must be unique and lexicographically ordered",
            ));
        }
        previous = Some(value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use cairn_protocol::{ContentId, EpisodeId, TaskId};
    use serde_json::{Value, json};

    use super::*;
    use crate::{
        AgentResolvedRuntimeModelArtifact, IntentRecoveryRequestV1, SirCapabilityManifestV1,
        SirProposalSubmissionV1, SirTaskBundleArtifact, SirTaskLimits,
    };

    fn recovery_input() -> IntentRecoveryInputV1 {
        let request: IntentRecoveryRequestV1 = serde_json::from_str(include_str!(
            "../../../fixtures/cuda-ascend/sir/compact-above-f32/v1/caller-intent.json"
        ))
        .expect("caller request");
        IntentRecoveryInputV1::new(
            TaskId::new(),
            ContentId::<SirTaskBundleArtifact>::derive(b"decision request task bundle")
                .expect("bundle identity"),
            request,
            SirCapabilityManifestV1::proposal_only(SirTaskLimits::default()),
        )
        .expect("recovery input")
    }

    fn submission_value(kind: &str, include_conflict: bool, caller_claim: &str) -> Value {
        let mut targets = vec![json!({"kind":"unknown","unknown":"output-order"})];
        if include_conflict {
            targets.insert(
                0,
                json!({"kind":"conflict","conflict":"output-order-conflict"}),
            );
        }
        json!({
            "schema_version":1,
            "observed_facts":[{
                "id":"atomic-slot-allocation",
                "statement":"The source allocates output slots atomically.",
                "citations":[{"path":"src/compact_above.cu","start_line":16,"end_line":20}]
            }],
            "hypotheses":[
                {
                    "id":"order-unspecified","layer":"observable-contract",
                    "claim":"Any permutation of qualifying values is acceptable.",
                    "domain":"Successful calls with sufficient output capacity.",
                    "supporting_evidence":[{"source":"caller-claim","claim":caller_claim}],
                    "counter_evidence":[]
                },
                {
                    "id":"stable-order","layer":"observable-contract",
                    "claim":"Qualifying values retain input-relative order.",
                    "domain":"Successful calls with sufficient output capacity.",
                    "supporting_evidence":[{"source":"caller-claim","claim":"copies-strictly-above"}],
                    "counter_evidence":[{"source":"observed-fact","observation":"atomic-slot-allocation"}]
                }
            ],
            "conflicts":[{
                "id":"output-order-conflict",
                "statement":"The two proposed output-order contracts are incompatible.",
                "claims":[
                    {"source":"hypothesis","hypothesis":"order-unspecified"},
                    {"source":"hypothesis","hypothesis":"stable-order"}
                ],
                "evidence":[{"source":"observed-fact","observation":"atomic-slot-allocation"}]
            }],
            "unknowns":[{
                "id":"output-order","kind":kind,
                "question":"Must output preserve input-relative order?",
                "evidence":[{"source":"observed-fact","observation":"atomic-slot-allocation"}]
            }],
            "invariants":[{
                "id":"copied-values",
                "statement":"Every copied value came from input.",
                "evidence":[{"source":"caller-claim","claim":"copies-strictly-above"}]
            }],
            "optimization_freedoms":[],
            "source_dispositions":[],
            "disambiguation_experiments":[{
                "id":"decide-output-order",
                "targets":targets,
                "plan":"Ask the actual task authority whether output ordering is observable.",
                "predictions":["Stable order selects stable-order.","Order-insensitive use selects order-unspecified."]
            }]
        })
    }

    fn proposal(
        recovery_input: &IntentRecoveryInputV1,
        submission: Value,
    ) -> IntentHypothesisSetProposalV1 {
        let submission: SirProposalSubmissionV1 =
            serde_json::from_value(submission).expect("strict proposal submission");
        IntentHypothesisSetProposalV1::new(
            recovery_input.identity().expect("input identity"),
            EpisodeId::new(),
            ContentId::<AgentResolvedRuntimeModelArtifact>::derive(b"recorded model")
                .expect("model identity"),
            submission,
        )
    }

    #[test]
    fn desired_semantics_closure_becomes_a_request_without_admission() {
        let input = recovery_input();
        let input_id = input.identity().expect("input identity");
        let proposal = proposal(
            &input,
            submission_value("desired-semantics", true, "copies-strictly-above"),
        );
        let proposal_id = proposal.identity().expect("proposal identity");
        let batch = derive_user_intent_decision_requests(proposal_id, &proposal, input_id, &input)
            .expect("decision request");
        assert_eq!(batch.requests().len(), 1);
        assert_eq!(batch.requests()[0].unknown().as_str(), "output-order");
        assert_eq!(batch.requests()[0].options().len(), 2);
        assert_eq!(
            batch.requests()[0].options()[0].hypothesis().as_str(),
            "order-unspecified"
        );
        let bytes = cairn_codec::to_vec(&batch).expect("canonical batch");
        assert_eq!(
            cairn_codec::from_slice::<IntentDecisionRequestBatchV1>(&bytes).expect("strict batch"),
            batch
        );
        assert!(batch.identity().is_ok());
    }

    #[test]
    fn incomplete_nonsemantic_and_dangling_inputs_fail_closed() {
        let input = recovery_input();
        let input_id = input.identity().expect("input identity");

        let missing_closure = proposal(
            &input,
            submission_value("desired-semantics", false, "copies-strictly-above"),
        );
        assert!(matches!(
            derive_user_intent_decision_requests(
                missing_closure.identity().expect("proposal identity"),
                &missing_closure,
                input_id,
                &input
            ),
            Err(IntentAdmissionError::IncompleteDecisionClosure { .. })
        ));

        let source_unknown = proposal(
            &input,
            submission_value("source-behavior", true, "copies-strictly-above"),
        );
        assert!(matches!(
            derive_user_intent_decision_requests(
                source_unknown.identity().expect("proposal identity"),
                &source_unknown,
                input_id,
                &input
            ),
            Err(IntentAdmissionError::NoDesiredSemanticsUnknown)
        ));

        let dangling = proposal(
            &input,
            submission_value("desired-semantics", true, "missing-caller-claim"),
        );
        assert!(
            derive_user_intent_decision_requests(
                dangling.identity().expect("proposal identity"),
                &dangling,
                input_id,
                &input
            )
            .is_err()
        );
    }
}
