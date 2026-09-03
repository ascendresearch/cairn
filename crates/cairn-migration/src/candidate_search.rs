//! Controller-owned durable state for one candidate search loop.
//!
//! A single episode is not a search. What makes this a loop is that a build observation returns to
//! the Controller, becomes the next immutable state, and only then decides whether another proposal
//! is asked for. The model owns each step's decision; it never owns the transition between steps,
//! and it cannot see that it is going in circles. This module is what does see it.
//!
//! Everything here is reconstructed from the loop's own event stream, so the answer to "what should
//! happen next" survives a Controller restart and is the same answer an auditor replaying the
//! stream would compute.

use cairn_execution::ExecutionReceiptArtifact;
use cairn_protocol::{
    AggregateId, AggregateKind, CommandId, ContentId, EventId, ObservedAtUnixMillis, SchemaName,
    SchemaVersion, StreamRevision, TaskId,
};
use cairn_record::{EventStore, EventStoreError, ExpectedRevision, NewEvent, StreamId};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::CandidateProposalArtifact;

const SEARCH_OPENED: &str = "migration.candidate-search-opened";
const PROPOSAL_RECORDED: &str = "migration.candidate-search-proposal-recorded";
const PROPOSAL_REPEATED: &str = "migration.candidate-search-proposal-repeated";
const SUBMISSION_MISSING: &str = "migration.candidate-search-submission-missing";
const BUILD_OBSERVED: &str = "migration.candidate-search-build-observed";

macro_rules! positive_count {
    ($name:ident, $subject:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(u32);

        impl $name {
            /// Creates a positive count.
            ///
            /// # Errors
            ///
            /// Zero is rejected because it cannot describe a loop that does anything.
            pub const fn new(value: u32) -> Result<Self, CandidateSearchError> {
                if value == 0 {
                    Err(CandidateSearchError::NonPositive($subject))
                } else {
                    Ok(Self(value))
                }
            }

            #[must_use]
            pub const fn get(self) -> u32 {
                self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

positive_count!(
    CandidateIterationLimit,
    "candidate iteration limit",
    "Maximum observed build iterations authorized for one candidate search loop.\n\nRepairable rate rises with iterations and then plateaus, so this is sized to cover the plateau\nrather than a single attempt. It is a product parameter, not a model decision."
);
positive_count!(
    CandidateEmptySubmissionLimit,
    "candidate empty submission limit",
    "Consecutive episodes that may end without any submission before the loop stops.\n\nAn episode that produces neither a typed proposal nor text is a failed attempt, not a finished\none: a reasoning model can spend its whole output budget before it submits anything."
);
positive_count!(
    CandidateRepeatWindow,
    "candidate repeat window",
    "How many already-built proposals are compared against a new submission."
);
positive_count!(
    CandidateBudgetNoticeThreshold,
    "candidate budget notice threshold",
    "Remaining iterations at or below which the Controller warns the actor.\n\nThe point is to let an actor converge rather than be cut off mid-plan."
);

impl Default for CandidateSearchPolicyV1 {
    /// Starting values, not measured ones.
    ///
    /// The architecture asks the iteration budget to cover the plateau in repairable rate rather
    /// than a single attempt, and nobody has measured where that plateau is for this target. These
    /// are deliberately modest and are meant to be replaced by configuration once a run has said
    /// something about the shape of the curve.
    fn default() -> Self {
        Self {
            iteration_limit: CandidateIterationLimit(6),
            empty_submission_limit: CandidateEmptySubmissionLimit(3),
            repeat_window: CandidateRepeatWindow(8),
            budget_notice_threshold: CandidateBudgetNoticeThreshold(2),
        }
    }
}

/// Iterations still authorized. Unlike the limits, zero is a meaningful value here.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CandidateIterationsRemaining(u32);

impl CandidateIterationsRemaining {
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Which iteration the loop is about to spend.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CandidateIterationOrdinal(u32);

impl CandidateIterationOrdinal {
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Frozen loop policy. It is Controller configuration and no episode can widen it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateSearchPolicyV1 {
    pub iteration_limit: CandidateIterationLimit,
    pub empty_submission_limit: CandidateEmptySubmissionLimit,
    pub repeat_window: CandidateRepeatWindow,
    pub budget_notice_threshold: CandidateBudgetNoticeThreshold,
}

/// One typed fact the Controller hands to the next episode about the loop itself.
///
/// These exist because the actor cannot observe any of them: it does not know how much budget is
/// left, that its last submission repeated an earlier one, or that its previous episodes produced
/// nothing at all. Returning the fact is what lets it change course instead of repeating.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "notice")]
pub enum CandidateSearchNoticeV1 {
    BuildBudgetLow {
        remaining: CandidateIterationsRemaining,
    },
    ProposalRepeated {
        proposal: ContentId<CandidateProposalArtifact>,
    },
    SubmissionMissing {
        consecutive: u32,
    },
}

/// What one build attempt observed about a candidate.
///
/// It is a search signal and never a verdict: `compiled` says the exact artifact was accepted by
/// the exact toolchain, which is silent about what the code means.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateBuildOutcomeV1 {
    proposal: ContentId<CandidateProposalArtifact>,
    receipt: ContentId<ExecutionReceiptArtifact>,
    compiled: bool,
}

impl CandidateBuildOutcomeV1 {
    /// Records what one build of one exact proposal observed.
    #[must_use]
    pub const fn new(
        proposal: ContentId<CandidateProposalArtifact>,
        receipt: ContentId<ExecutionReceiptArtifact>,
        compiled: bool,
    ) -> Self {
        Self {
            proposal,
            receipt,
            compiled,
        }
    }

    #[must_use]
    pub const fn proposal(&self) -> ContentId<CandidateProposalArtifact> {
        self.proposal
    }

    #[must_use]
    pub const fn receipt(&self) -> ContentId<ExecutionReceiptArtifact> {
        self.receipt
    }

    #[must_use]
    pub const fn compiled(&self) -> bool {
        self.compiled
    }
}

/// The exact proposal a build refuted, and the receipt that refuted it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateSearchParentV1 {
    proposal: ContentId<CandidateProposalArtifact>,
    receipt: ContentId<ExecutionReceiptArtifact>,
}

impl CandidateSearchParentV1 {
    #[must_use]
    pub const fn proposal(&self) -> ContentId<CandidateProposalArtifact> {
        self.proposal
    }

    #[must_use]
    pub const fn receipt(&self) -> ContentId<ExecutionReceiptArtifact> {
        self.receipt
    }
}

/// Why a loop stopped without a compiling candidate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateSearchStopV1 {
    IterationBudgetExhausted,
    SubmissionMissingLimitReached,
}

/// Terminal state of this loop. It is a search outcome and never a migration verdict.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum CandidateSearchTerminalV1 {
    /// One proposal reached a build that compiled. It says nothing about what the code means.
    Compiled {
        proposal: ContentId<CandidateProposalArtifact>,
        receipt: ContentId<ExecutionReceiptArtifact>,
    },
    Stopped {
        stop: CandidateSearchStopV1,
        last: Option<CandidateSearchParentV1>,
    },
}

/// Progress carried across iterations of one open loop.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateSearchProgressV1 {
    policy: CandidateSearchPolicyV1,
    iterations_used: u32,
    consecutive_missing: u32,
    built: Vec<ContentId<CandidateProposalArtifact>>,
    parent: Option<CandidateSearchParentV1>,
    notice: Option<CandidateSearchNoticeV1>,
}

impl CandidateSearchProgressV1 {
    #[must_use]
    pub const fn policy(&self) -> CandidateSearchPolicyV1 {
        self.policy
    }

    #[must_use]
    pub const fn parent(&self) -> Option<CandidateSearchParentV1> {
        self.parent
    }

    /// Iterations still authorized, saturating at zero.
    #[must_use]
    pub const fn remaining(&self) -> CandidateIterationsRemaining {
        CandidateIterationsRemaining(
            self.policy
                .iteration_limit
                .get()
                .saturating_sub(self.iterations_used),
        )
    }

    const fn next_ordinal(&self) -> CandidateIterationOrdinal {
        CandidateIterationOrdinal(self.iterations_used.saturating_add(1))
    }

    /// The one typed fact the next episode should be told about the loop, if there is one.
    ///
    /// A budget warning is derived rather than recorded: it is a fact about the policy and the
    /// count, and storing a second copy would let the copy disagree with the count. A more
    /// specific notice wins, because an actor that just repeated itself needs to hear that first.
    #[must_use]
    pub fn notice(&self) -> Option<CandidateSearchNoticeV1> {
        if self.notice.is_some() {
            return self.notice;
        }
        let remaining = self.remaining();
        (remaining.get() <= self.policy.budget_notice_threshold.get())
            .then_some(CandidateSearchNoticeV1::BuildBudgetLow { remaining })
    }
}

/// Durable loop state reconstructed solely from current-V1 events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateSearchStateV1 {
    NotFound,
    AwaitingProposal(CandidateSearchProgressV1),
    AwaitingBuild {
        progress: CandidateSearchProgressV1,
        proposal: ContentId<CandidateProposalArtifact>,
    },
    Terminal(CandidateSearchTerminalV1),
}

/// One exact next action selected from durable state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateSearchNextActionV1 {
    None,
    RequestProposal {
        iteration: CandidateIterationOrdinal,
        remaining: CandidateIterationsRemaining,
        parent: Option<CandidateSearchParentV1>,
        notice: Option<CandidateSearchNoticeV1>,
    },
    RequestBuild {
        iteration: CandidateIterationOrdinal,
        proposal: ContentId<CandidateProposalArtifact>,
    },
    Terminal(CandidateSearchTerminalV1),
}

impl CandidateSearchStateV1 {
    /// Selects the one action implied by recovered durable state.
    #[must_use]
    pub fn next_action(&self) -> CandidateSearchNextActionV1 {
        match self {
            Self::NotFound => CandidateSearchNextActionV1::None,
            Self::AwaitingProposal(progress) => CandidateSearchNextActionV1::RequestProposal {
                iteration: progress.next_ordinal(),
                remaining: progress.remaining(),
                parent: progress.parent,
                notice: progress.notice(),
            },
            Self::AwaitingBuild {
                progress, proposal, ..
            } => CandidateSearchNextActionV1::RequestBuild {
                iteration: progress.next_ordinal(),
                proposal: *proposal,
            },
            Self::Terminal(outcome) => CandidateSearchNextActionV1::Terminal(*outcome),
        }
    }
}

/// Aggregate boundary for one task's candidate search loop.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateSearchLoopV1 {
    task_id: TaskId,
    stream: StreamId,
}

impl CandidateSearchLoopV1 {
    /// Creates the task-owned aggregate and its private record-stream identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the aggregate kind or task identity cannot be represented.
    pub fn new(task_id: TaskId) -> Result<Self, CandidateSearchError> {
        Ok(Self {
            task_id,
            stream: StreamId {
                kind: AggregateKind::new("candidate-search-loop")
                    .map_err(|error| invalid(&error.to_string()))?,
                id: AggregateId::new(task_id.to_string())
                    .map_err(|error| invalid(&error.to_string()))?,
            },
        })
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OpenedPayload {
    task_id: TaskId,
    policy: CandidateSearchPolicyV1,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProposalPayload {
    proposal: ContentId<CandidateProposalArtifact>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MissingPayload {
    consecutive: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BuildObservedPayload {
    proposal: ContentId<CandidateProposalArtifact>,
    receipt: ContentId<ExecutionReceiptArtifact>,
    compiled: bool,
    iterations_used: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoppedPayload {
    stop: CandidateSearchStopV1,
    last: Option<CandidateSearchParentV1>,
}

struct Projection {
    state: CandidateSearchStateV1,
    revision: Option<StreamRevision>,
    last_event_id: Option<EventId>,
}

/// Opens one candidate search loop under a frozen policy.
///
/// # Errors
///
/// Rejects a loop that already exists and returns store, codec, or history errors.
pub fn open_candidate_search<E: EventStore>(
    events: &mut E,
    search: &CandidateSearchLoopV1,
    policy: CandidateSearchPolicyV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<CandidateSearchStateV1, CandidateSearchError> {
    let projection = project(events, search)?;
    if !matches!(projection.state, CandidateSearchStateV1::NotFound) {
        return Err(CandidateSearchError::InvalidTransition);
    }
    append(
        events,
        search,
        &projection,
        command_id,
        observed_at,
        SEARCH_OPENED,
        &OpenedPayload {
            task_id: search.task_id,
            policy,
        },
    )?;
    Ok(project(events, search)?.state)
}

/// Records one proposal, or the fact that it repeats a proposal this loop already built.
///
/// A repeat is not executed. Building the same bytes twice would spend an iteration to learn what
/// the loop already knows, and the actor cannot tell it is repeating unless it is told.
///
/// # Errors
///
/// Rejects a proposal recorded outside an open loop awaiting one.
pub fn record_candidate_proposal<E: EventStore>(
    events: &mut E,
    search: &CandidateSearchLoopV1,
    proposal: ContentId<CandidateProposalArtifact>,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<CandidateSearchStateV1, CandidateSearchError> {
    let projection = project(events, search)?;
    let CandidateSearchStateV1::AwaitingProposal(progress) = &projection.state else {
        return Err(CandidateSearchError::InvalidTransition);
    };
    let schema = if progress.built.contains(&proposal) {
        PROPOSAL_REPEATED
    } else {
        PROPOSAL_RECORDED
    };
    append(
        events,
        search,
        &projection,
        command_id,
        observed_at,
        schema,
        &ProposalPayload { proposal },
    )?;
    Ok(project(events, search)?.state)
}

/// Records an episode that ended without any submission, and stops the loop at its limit.
///
/// # Errors
///
/// Rejects a missing submission recorded outside an open loop awaiting a proposal.
pub fn record_missing_submission<E: EventStore>(
    events: &mut E,
    search: &CandidateSearchLoopV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<CandidateSearchStateV1, CandidateSearchError> {
    let projection = project(events, search)?;
    let CandidateSearchStateV1::AwaitingProposal(progress) = &projection.state else {
        return Err(CandidateSearchError::InvalidTransition);
    };
    let consecutive = progress.consecutive_missing.saturating_add(1);
    append(
        events,
        search,
        &projection,
        command_id,
        observed_at,
        SUBMISSION_MISSING,
        &MissingPayload { consecutive },
    )?;
    Ok(project(events, search)?.state)
}

/// Folds one build receipt back into durable state as soon as it arrives.
///
/// The state advances here rather than at the end of the episode, because the end of an episode is
/// exactly where a budget-limited actor is least likely to arrive.
///
/// # Errors
///
/// Rejects an observation for a proposal this loop is not currently building.
pub fn record_candidate_build_observation<E: EventStore>(
    events: &mut E,
    search: &CandidateSearchLoopV1,
    proposal: ContentId<CandidateProposalArtifact>,
    receipt: ContentId<ExecutionReceiptArtifact>,
    compiled: bool,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<CandidateSearchStateV1, CandidateSearchError> {
    let projection = project(events, search)?;
    let CandidateSearchStateV1::AwaitingBuild {
        progress,
        proposal: building,
    } = &projection.state
    else {
        return Err(CandidateSearchError::InvalidTransition);
    };
    if *building != proposal {
        return Err(CandidateSearchError::InvalidTransition);
    }
    let iterations_used = progress.iterations_used.saturating_add(1);
    append(
        events,
        search,
        &projection,
        command_id,
        observed_at,
        BUILD_OBSERVED,
        &BuildObservedPayload {
            proposal,
            receipt,
            compiled,
            iterations_used,
        },
    )?;
    Ok(project(events, search)?.state)
}

/// Recovers durable loop state without changing it.
///
/// # Errors
///
/// Returns store, codec, or invalid-history errors.
pub fn recover_candidate_search<E: EventStore>(
    events: &E,
    search: &CandidateSearchLoopV1,
) -> Result<CandidateSearchStateV1, CandidateSearchError> {
    Ok(project(events, search)?.state)
}

fn project<E: EventStore>(
    events: &E,
    search: &CandidateSearchLoopV1,
) -> Result<Projection, CandidateSearchError> {
    let history = events.read_stream(&search.stream, None)?;
    let mut state = CandidateSearchStateV1::NotFound;
    let mut last_event_id = None;
    for event in &history {
        if event.schema_version != schema_v1()? {
            return Err(invalid_history("non-V1 candidate search event"));
        }
        if event.parent_event_id != last_event_id {
            return Err(invalid_history("candidate search causal parent changed"));
        }
        state = apply(
            search.task_id,
            state,
            event.schema_name.as_str(),
            &event.payload,
        )?;
        last_event_id = Some(event.event_id);
    }
    Ok(Projection {
        state,
        revision: history
            .last()
            .map(|event| StreamRevision::new(event.sequence.get()))
            .transpose()
            .map_err(|error| invalid(&error.to_string()))?,
        last_event_id,
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "the event fold keeps every legal current-V1 transition visible in one exhaustive match"
)]
fn apply(
    task_id: TaskId,
    state: CandidateSearchStateV1,
    schema: &str,
    bytes: &[u8],
) -> Result<CandidateSearchStateV1, CandidateSearchError> {
    match (state, schema) {
        (CandidateSearchStateV1::NotFound, SEARCH_OPENED) => {
            let payload: OpenedPayload = decode(bytes)?;
            if payload.task_id != task_id {
                return Err(invalid_history("candidate search opened for another task"));
            }
            Ok(CandidateSearchStateV1::AwaitingProposal(
                CandidateSearchProgressV1 {
                    policy: payload.policy,
                    iterations_used: 0,
                    consecutive_missing: 0,
                    built: Vec::new(),
                    parent: None,
                    notice: None,
                },
            ))
        }
        (CandidateSearchStateV1::AwaitingProposal(progress), PROPOSAL_RECORDED) => {
            let payload: ProposalPayload = decode(bytes)?;
            if progress.built.contains(&payload.proposal) {
                return Err(invalid_history("recorded proposal was already built"));
            }
            Ok(CandidateSearchStateV1::AwaitingBuild {
                progress: CandidateSearchProgressV1 {
                    consecutive_missing: 0,
                    notice: None,
                    ..progress
                },
                proposal: payload.proposal,
            })
        }
        (CandidateSearchStateV1::AwaitingProposal(progress), PROPOSAL_REPEATED) => {
            let payload: ProposalPayload = decode(bytes)?;
            if !progress.built.contains(&payload.proposal) {
                return Err(invalid_history("repeated proposal was never built"));
            }
            Ok(CandidateSearchStateV1::AwaitingProposal(
                CandidateSearchProgressV1 {
                    consecutive_missing: 0,
                    notice: Some(CandidateSearchNoticeV1::ProposalRepeated {
                        proposal: payload.proposal,
                    }),
                    ..progress
                },
            ))
        }
        (CandidateSearchStateV1::AwaitingProposal(progress), SUBMISSION_MISSING) => {
            let payload: MissingPayload = decode(bytes)?;
            if payload.consecutive != progress.consecutive_missing.saturating_add(1) {
                return Err(invalid_history("missing-submission count skipped a value"));
            }
            if payload.consecutive >= progress.policy.empty_submission_limit.get() {
                return Ok(CandidateSearchStateV1::Terminal(
                    CandidateSearchTerminalV1::Stopped {
                        stop: CandidateSearchStopV1::SubmissionMissingLimitReached,
                        last: progress.parent,
                    },
                ));
            }
            Ok(CandidateSearchStateV1::AwaitingProposal(
                CandidateSearchProgressV1 {
                    consecutive_missing: payload.consecutive,
                    notice: Some(CandidateSearchNoticeV1::SubmissionMissing {
                        consecutive: payload.consecutive,
                    }),
                    ..progress
                },
            ))
        }
        (CandidateSearchStateV1::AwaitingBuild { progress, proposal }, BUILD_OBSERVED) => {
            let payload: BuildObservedPayload = decode(bytes)?;
            if payload.proposal != proposal
                || payload.iterations_used != progress.iterations_used.saturating_add(1)
            {
                return Err(invalid_history(
                    "build observation left its exact iteration",
                ));
            }
            if payload.compiled {
                return Ok(CandidateSearchStateV1::Terminal(
                    CandidateSearchTerminalV1::Compiled {
                        proposal,
                        receipt: payload.receipt,
                    },
                ));
            }
            let parent = CandidateSearchParentV1 {
                proposal,
                receipt: payload.receipt,
            };
            if payload.iterations_used >= progress.policy.iteration_limit.get() {
                return Ok(CandidateSearchStateV1::Terminal(
                    CandidateSearchTerminalV1::Stopped {
                        stop: CandidateSearchStopV1::IterationBudgetExhausted,
                        last: Some(parent),
                    },
                ));
            }
            let window = progress.policy.repeat_window.get() as usize;
            let mut built = progress.built;
            built.push(proposal);
            if built.len() > window {
                built.remove(0);
            }
            Ok(CandidateSearchStateV1::AwaitingProposal(
                CandidateSearchProgressV1 {
                    iterations_used: payload.iterations_used,
                    consecutive_missing: 0,
                    built,
                    parent: Some(parent),
                    notice: None,
                    ..progress
                },
            ))
        }
        (CandidateSearchStateV1::Terminal(_), _) => {
            Err(invalid_history("candidate search advanced past a terminal"))
        }
        _ => Err(invalid_history("candidate search event is out of order")),
    }
}

fn append<E: EventStore, P: Serialize>(
    events: &mut E,
    search: &CandidateSearchLoopV1,
    projection: &Projection,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
    schema: &str,
    payload: &P,
) -> Result<(), CandidateSearchError> {
    let event = NewEvent {
        schema_name: SchemaName::new(schema).map_err(|error| invalid(&error.to_string()))?,
        schema_version: schema_v1()?,
        parent_event_id: projection.last_event_id,
        observed_at_unix_ms: observed_at.get(),
        payload: cairn_codec::to_vec(payload)
            .map_err(|error| CandidateSearchError::Codec(error.to_string()))?,
    };
    events.append(
        &search.stream,
        expected(projection.revision),
        command_id,
        &[event],
    )?;
    Ok(())
}

fn schema_v1() -> Result<SchemaVersion, CandidateSearchError> {
    SchemaVersion::new(1).map_err(|error| invalid(&error.to_string()))
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, CandidateSearchError> {
    cairn_codec::from_slice(bytes).map_err(|error| CandidateSearchError::Codec(error.to_string()))
}

const fn expected(revision: Option<StreamRevision>) -> ExpectedRevision {
    match revision {
        Some(revision) => ExpectedRevision::Exact(revision),
        None => ExpectedRevision::NoStream,
    }
}

fn invalid(message: &str) -> CandidateSearchError {
    CandidateSearchError::InvalidHistory(message.to_owned())
}

fn invalid_history(message: &str) -> CandidateSearchError {
    invalid(message)
}

/// Why a candidate search transition could not be recorded or recovered.
#[derive(Debug, Error)]
pub enum CandidateSearchError {
    #[error("{0} must be positive")]
    NonPositive(&'static str),
    #[error("candidate search transition is not legal from the current durable state")]
    InvalidTransition,
    #[error("candidate search history is invalid: {0}")]
    InvalidHistory(String),
    #[error("candidate search codec failure: {0}")]
    Codec(String),
    #[error(transparent)]
    Event(#[from] EventStoreError),
}

#[cfg(test)]
mod tests {
    use cairn_store_sqlite::SqliteEventStore;

    use super::*;

    fn policy(
        iterations: u32,
        missing: u32,
        window: u32,
        threshold: u32,
    ) -> CandidateSearchPolicyV1 {
        CandidateSearchPolicyV1 {
            iteration_limit: CandidateIterationLimit::new(iterations).expect("iteration limit"),
            empty_submission_limit: CandidateEmptySubmissionLimit::new(missing)
                .expect("missing submission limit"),
            repeat_window: CandidateRepeatWindow::new(window).expect("repeat window"),
            budget_notice_threshold: CandidateBudgetNoticeThreshold::new(threshold)
                .expect("budget notice threshold"),
        }
    }

    fn at() -> ObservedAtUnixMillis {
        ObservedAtUnixMillis::new(1)
    }

    fn proposal(label: &[u8]) -> ContentId<CandidateProposalArtifact> {
        ContentId::derive(label).expect("proposal identity")
    }

    fn receipt(label: &[u8]) -> ContentId<ExecutionReceiptArtifact> {
        ContentId::derive(label).expect("receipt identity")
    }

    struct Fixture {
        events: SqliteEventStore,
        search: CandidateSearchLoopV1,
    }

    impl Fixture {
        fn open(policy: CandidateSearchPolicyV1) -> Self {
            let mut events = SqliteEventStore::in_memory().expect("in-memory event store");
            let search = CandidateSearchLoopV1::new(TaskId::new()).expect("search loop");
            open_candidate_search(&mut events, &search, policy, &CommandId::new(), at())
                .expect("open");
            Self { events, search }
        }

        fn propose(&mut self, label: &[u8]) -> CandidateSearchStateV1 {
            record_candidate_proposal(
                &mut self.events,
                &self.search,
                proposal(label),
                &CommandId::new(),
                at(),
            )
            .expect("record proposal")
        }

        fn build(&mut self, label: &[u8], compiled: bool) -> CandidateSearchStateV1 {
            record_candidate_build_observation(
                &mut self.events,
                &self.search,
                proposal(label),
                receipt(label),
                compiled,
                &CommandId::new(),
                at(),
            )
            .expect("record build")
        }

        fn missing(&mut self) -> CandidateSearchStateV1 {
            record_missing_submission(&mut self.events, &self.search, &CommandId::new(), at())
                .expect("record missing submission")
        }
    }

    #[test]
    fn a_build_that_compiles_ends_the_search_naming_what_it_observed() {
        let mut fixture = Fixture::open(policy(4, 2, 4, 1));
        fixture.propose(b"first");

        let state = fixture.build(b"first", true);

        assert_eq!(
            state.next_action(),
            CandidateSearchNextActionV1::Terminal(CandidateSearchTerminalV1::Compiled {
                proposal: proposal(b"first"),
                receipt: receipt(b"first"),
            })
        );
    }

    // The whole point of the loop: a refused build hands the next episode the exact proposal that
    // failed and the exact receipt that refused it, rather than starting over from nothing.
    #[test]
    fn a_refused_build_asks_for_a_revision_carrying_the_receipt_that_refused_it() {
        let mut fixture = Fixture::open(policy(4, 2, 4, 1));
        fixture.propose(b"first");

        let state = fixture.build(b"first", false);

        let CandidateSearchNextActionV1::RequestProposal {
            iteration,
            remaining,
            parent,
            notice,
        } = state.next_action()
        else {
            panic!("a refused build asks for another proposal");
        };
        assert_eq!(iteration.get(), 2);
        assert_eq!(remaining.get(), 3);
        assert_eq!(notice, None);
        let parent = parent.expect("the refused proposal is the parent");
        assert_eq!(parent.proposal(), proposal(b"first"));
        assert_eq!(parent.receipt(), receipt(b"first"));
    }

    #[test]
    fn the_iteration_budget_stops_the_search_and_keeps_the_last_refusal() {
        let mut fixture = Fixture::open(policy(2, 2, 4, 1));
        fixture.propose(b"first");
        fixture.build(b"first", false);
        fixture.propose(b"second");

        let state = fixture.build(b"second", false);

        assert_eq!(
            state.next_action(),
            CandidateSearchNextActionV1::Terminal(CandidateSearchTerminalV1::Stopped {
                stop: CandidateSearchStopV1::IterationBudgetExhausted,
                last: Some(CandidateSearchParentV1 {
                    proposal: proposal(b"second"),
                    receipt: receipt(b"second"),
                }),
            })
        );
    }

    // A model cannot see that it is going in circles. Building the same bytes again would spend an
    // iteration to learn what the loop already knows, so the repeat is refused and reported.
    #[test]
    fn a_repeated_proposal_is_not_built_and_is_handed_back_as_a_notice() {
        let mut fixture = Fixture::open(policy(4, 2, 4, 1));
        fixture.propose(b"first");
        fixture.build(b"first", false);

        let state = fixture.propose(b"first");

        let CandidateSearchNextActionV1::RequestProposal {
            iteration, notice, ..
        } = state.next_action()
        else {
            panic!("a repeat asks for another proposal rather than a build");
        };
        assert_eq!(
            notice,
            Some(CandidateSearchNoticeV1::ProposalRepeated {
                proposal: proposal(b"first")
            })
        );
        // The repeat cost no iteration: it is still the second one that is about to be spent.
        assert_eq!(iteration.get(), 2);
    }

    #[test]
    fn missing_submissions_accumulate_until_the_limit_and_a_proposal_clears_them() {
        let mut fixture = Fixture::open(policy(4, 3, 4, 1));

        let first = fixture.missing();
        assert!(matches!(
            first.next_action(),
            CandidateSearchNextActionV1::RequestProposal {
                notice: Some(CandidateSearchNoticeV1::SubmissionMissing { consecutive: 1 }),
                ..
            }
        ));
        fixture.propose(b"first");
        fixture.build(b"first", false);

        // The run was broken by a real submission, so counting starts again rather than carrying.
        let after = fixture.missing();
        assert!(matches!(
            after.next_action(),
            CandidateSearchNextActionV1::RequestProposal {
                notice: Some(CandidateSearchNoticeV1::SubmissionMissing { consecutive: 1 }),
                ..
            }
        ));
        fixture.missing();
        let stopped = fixture.missing();
        assert_eq!(
            stopped.next_action(),
            CandidateSearchNextActionV1::Terminal(CandidateSearchTerminalV1::Stopped {
                stop: CandidateSearchStopV1::SubmissionMissingLimitReached,
                last: Some(CandidateSearchParentV1 {
                    proposal: proposal(b"first"),
                    receipt: receipt(b"first"),
                }),
            })
        );
    }

    #[test]
    fn the_actor_is_warned_before_the_budget_runs_out_rather_than_being_cut_off() {
        let mut fixture = Fixture::open(policy(3, 2, 4, 1));
        fixture.propose(b"first");
        fixture.build(b"first", false);
        fixture.propose(b"second");

        let state = fixture.build(b"second", false);

        let CandidateSearchNextActionV1::RequestProposal {
            remaining, notice, ..
        } = state.next_action()
        else {
            panic!("one iteration is left");
        };
        assert_eq!(remaining.get(), 1);
        assert_eq!(
            notice,
            Some(CandidateSearchNoticeV1::BuildBudgetLow { remaining })
        );
    }

    // The loop's position is not held in memory by whoever is driving it. A reader that has only
    // the stream reaches the same state and the same next action.
    #[test]
    fn the_search_position_is_reconstructed_from_events_alone() {
        let mut fixture = Fixture::open(policy(4, 2, 4, 1));
        fixture.propose(b"first");
        fixture.build(b"first", false);
        let live = fixture.propose(b"second");

        let recovered =
            recover_candidate_search(&fixture.events, &fixture.search).expect("recover");

        assert_eq!(recovered, live);
        assert_eq!(
            recovered.next_action(),
            CandidateSearchNextActionV1::RequestBuild {
                iteration: CandidateIterationOrdinal(2),
                proposal: proposal(b"second"),
            }
        );
    }

    #[test]
    fn an_observation_for_a_proposal_the_loop_is_not_building_is_refused() {
        let mut fixture = Fixture::open(policy(4, 2, 4, 1));
        fixture.propose(b"first");

        let error = record_candidate_build_observation(
            &mut fixture.events,
            &fixture.search,
            proposal(b"other"),
            receipt(b"other"),
            true,
            &CommandId::new(),
            at(),
        )
        .expect_err("an observation cannot name a proposal this loop never authorized");

        assert!(matches!(error, CandidateSearchError::InvalidTransition));
    }
}
