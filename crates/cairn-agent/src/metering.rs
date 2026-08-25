//! Durable pre-effect reservation and post-effect receipt capabilities for named external meters.
//!
//! Metered actions deliberately have a distinct identity domain from tool operations:
//!
//! ```compile_fail
//! use cairn_protocol::{MeteredActionId, OperationId};
//!
//! let operation_id: OperationId = MeteredActionId::new();
//! ```

use std::collections::HashSet;

use cairn_protocol::{
    AggregateId, AggregateKind, CommandId, EpisodeId, EventId, MeteredActionId,
    ObservedAtUnixMillis, SchemaName, SchemaVersion, StreamRevision,
};
use cairn_record::{
    EventEnvelope, EventStore, EventStoreError, ExpectedRevision, NewEvent, StreamId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{AgentEpisode, ExternalMeterName, ExternalReceiptReference};

const RESERVED: &str = "agent.episode-meter-reserved";
const DENIED: &str = "agent.episode-meter-denied";
const STARTED: &str = "agent.metered-action-started";
const RECEIPTED: &str = "agent.metered-action-receipted";

/// Count in the unit named by an [`ExternalMeterName`].
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ExternalMeteredUnits(u64);

impl ExternalMeteredUnits {
    /// Creates a unit count. Zero is valid for receipts and limits.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the unit count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Configured reservation ceiling for one external meter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EpisodeExternalMeterLimit {
    meter: ExternalMeterName,
    units: ExternalMeteredUnits,
}

impl EpisodeExternalMeterLimit {
    /// Creates one meter-specific limit.
    #[must_use]
    pub const fn new(meter: ExternalMeterName, units: ExternalMeteredUnits) -> Self {
        Self { meter, units }
    }

    /// Returns the configured meter.
    #[must_use]
    pub const fn meter(&self) -> &ExternalMeterName {
        &self.meter
    }

    /// Returns the reservation ceiling.
    #[must_use]
    pub const fn units(&self) -> ExternalMeteredUnits {
        self.units
    }
}

/// Receipt returned by an externally metered capability.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalMeteringReceipt {
    meter: ExternalMeterName,
    charged_units: ExternalMeteredUnits,
    external_reference: Option<ExternalReceiptReference>,
}

impl ExternalMeteringReceipt {
    /// Creates a receipt in the reservation's declared unit.
    #[must_use]
    pub const fn new(
        meter: ExternalMeterName,
        charged_units: ExternalMeteredUnits,
        external_reference: Option<ExternalReceiptReference>,
    ) -> Self {
        Self {
            meter,
            charged_units,
            external_reference,
        }
    }

    /// Returns the meter identity.
    #[must_use]
    pub const fn meter(&self) -> &ExternalMeterName {
        &self.meter
    }

    /// Returns actual charged units.
    #[must_use]
    pub const fn charged_units(&self) -> ExternalMeteredUnits {
        self.charged_units
    }

    /// Returns the provider's receipt reference when supplied.
    #[must_use]
    pub const fn external_reference(&self) -> Option<&ExternalReceiptReference> {
        self.external_reference.as_ref()
    }
}

/// Durable reservation identity and amount.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeterReservation {
    episode_id: EpisodeId,
    action_id: MeteredActionId,
    meter: ExternalMeterName,
    reserved_units: ExternalMeteredUnits,
}

impl MeterReservation {
    /// Returns the owning episode.
    #[must_use]
    pub const fn episode_id(&self) -> EpisodeId {
        self.episode_id
    }

    /// Returns the metered action identity.
    #[must_use]
    pub const fn action_id(&self) -> MeteredActionId {
        self.action_id
    }

    /// Returns the meter identity.
    #[must_use]
    pub const fn meter(&self) -> &ExternalMeterName {
        &self.meter
    }

    /// Returns units charged against the budget before execution.
    #[must_use]
    pub const fn reserved_units(&self) -> ExternalMeteredUnits {
        self.reserved_units
    }
}

/// One-shot proof that an external action fits the episode's configured meter budget.
pub struct MeteredActionAuthority {
    reservation: MeterReservation,
    reservation_event_id: EventId,
    action_stream: StreamId,
}

impl MeteredActionAuthority {
    /// Returns the durable reservation represented by this one-shot authority.
    #[must_use]
    pub const fn reservation(&self) -> &MeterReservation {
        &self.reservation
    }
}

/// Proof that the metered action may have happened and must never be blindly repeated.
pub struct StartedMeteredAction {
    reservation: MeterReservation,
    action_stream: StreamId,
    revision: StreamRevision,
    started_event_id: EventId,
}

impl StartedMeteredAction {
    /// Returns the reservation whose external execution is now in doubt.
    #[must_use]
    pub const fn reservation(&self) -> &MeterReservation {
        &self.reservation
    }
}

/// Result of the pre-effect reservation decision.
pub enum MeterReservationOutcome {
    /// Budget was disabled or sufficient and one-shot authority was granted.
    Reserved(MeteredActionAuthority),
    /// Enabled policy rejected an unconfigured meter or exhausted limit.
    Denied {
        /// Requested reservation.
        reservation: MeterReservation,
        /// Configured ceiling, absent when the enabled policy did not list the meter.
        limit: Option<ExternalMeteredUnits>,
    },
}

/// Recovered state of one external metered action.
pub enum MeteredActionState {
    /// No reservation decision exists.
    NotFound,
    /// Reservation was denied before external authority existed.
    Denied {
        reservation: MeterReservation,
        limit: Option<ExternalMeteredUnits>,
    },
    /// Reservation is durable and safe to mark started.
    ReadyToStart(MeteredActionAuthority),
    /// Started was durable; the action must not be repeated, but a reconciled receipt may commit.
    InDoubt(StartedMeteredAction),
    /// A bounded receipt is durable.
    Receipted {
        reservation: MeterReservation,
        receipt: ExternalMeteringReceipt,
    },
}

/// Metering saga or configuration failure.
#[derive(Debug, Error)]
pub enum MeteringCoordinatorError {
    #[error(transparent)]
    Event(#[from] EventStoreError),
    #[error("invalid episode metering history: {0}")]
    InvalidHistory(String),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ReservationDecision {
    Reserved,
    Denied,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReservationPayload {
    episode_id: EpisodeId,
    action_id: MeteredActionId,
    meter: ExternalMeterName,
    reserved_units: ExternalMeteredUnits,
    decision: ReservationDecision,
    limit: Option<ExternalMeteredUnits>,
}

#[derive(Clone)]
struct ReservationEntry {
    payload: ReservationPayload,
    event_id: EventId,
    command_id: CommandId,
    observed_at_unix_ms: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StartedPayload {
    episode_id: EpisodeId,
    action_id: MeteredActionId,
    meter: ExternalMeterName,
    reserved_units: ExternalMeteredUnits,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReceiptPayload {
    episode_id: EpisodeId,
    action_id: MeteredActionId,
    receipt: ExternalMeteringReceipt,
}

/// Reserves a maximum charge before any external effect receives authority.
///
/// # Errors
///
/// Returns [`MeteringCoordinatorError`] when the episode/configuration history is invalid, the
/// reservation conflicts with a replayed command, or the durable decision cannot be committed.
#[allow(clippy::too_many_arguments)]
pub fn reserve_metered_action<E: EventStore>(
    events: &mut E,
    episode: &AgentEpisode,
    action_id: MeteredActionId,
    meter: ExternalMeterName,
    reserved_units: ExternalMeteredUnits,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<MeterReservationOutcome, MeteringCoordinatorError> {
    if reserved_units.get() == 0 {
        return invalid("meter reservation must be greater than zero");
    }
    let snapshot = crate::episode::recover_budget_snapshot(events, episode)
        .map_err(|error| MeteringCoordinatorError::InvalidHistory(error.to_string()))?;
    if snapshot.completed {
        return invalid("completed episode cannot reserve metered actions");
    }
    let ledger = ledger_stream(episode.episode_id())?;
    let history = events.read_stream(&ledger, None)?;
    let entries = project_ledger(&history, episode.episode_id())?;
    let requested = MeterReservation {
        episode_id: episode.episode_id(),
        action_id,
        meter: meter.clone(),
        reserved_units,
    };
    validate_ledger(&entries, snapshot.budget.external_meter_limits.as_deref())?;
    if let Some(existing) = entries
        .iter()
        .find(|entry| entry.payload.action_id == action_id)
    {
        if existing.command_id != *command_id
            || existing.observed_at_unix_ms != observed_at.get()
            || existing.payload.meter != meter
            || existing.payload.reserved_units != reserved_units
        {
            return invalid("replayed meter reservation differs from durable decision");
        }
        return outcome_from_entry(events, existing, requested);
    }

    let (decision, limit) = reservation_decision(
        snapshot.budget.external_meter_limits.as_deref(),
        &entries,
        &meter,
        reserved_units,
    )?;
    let payload = ReservationPayload {
        episode_id: episode.episode_id(),
        action_id,
        meter,
        reserved_units,
        decision,
        limit,
    };
    let event = fact(
        match decision {
            ReservationDecision::Reserved => RESERVED,
            ReservationDecision::Denied => DENIED,
        },
        history.last().map(|event| event.event_id),
        observed_at,
        &payload,
    )?;
    let expected = match history.last() {
        None => ExpectedRevision::NoStream,
        Some(last) => ExpectedRevision::Exact(revision(last.sequence)?),
    };
    let appended = events.append(&ledger, expected, command_id, &[event])?;
    let entry = ReservationEntry {
        payload,
        event_id: appended.event_ids[0],
        command_id: *command_id,
        observed_at_unix_ms: observed_at.get(),
    };
    outcome_from_entry(events, &entry, requested)
}

/// Marks a reserved action started before the external effect is attempted.
///
/// # Errors
///
/// Returns [`MeteringCoordinatorError`] when the start fact cannot be encoded or committed.
pub fn begin_metered_action<E: EventStore>(
    events: &mut E,
    authority: MeteredActionAuthority,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<StartedMeteredAction, MeteringCoordinatorError> {
    let payload = StartedPayload {
        episode_id: authority.reservation.episode_id,
        action_id: authority.reservation.action_id,
        meter: authority.reservation.meter.clone(),
        reserved_units: authority.reservation.reserved_units,
    };
    let event = fact(
        STARTED,
        Some(authority.reservation_event_id),
        observed_at,
        &payload,
    )?;
    let appended = events.append(
        &authority.action_stream,
        ExpectedRevision::NoStream,
        command_id,
        &[event],
    )?;
    Ok(StartedMeteredAction {
        reservation: authority.reservation,
        action_stream: authority.action_stream,
        revision: revision(appended.last_sequence)?,
        started_event_id: appended.event_ids[0],
    })
}

/// Commits the external receipt, bounded by the pre-effect reservation.
///
/// # Errors
///
/// Returns [`MeteringCoordinatorError`] when the receipt uses another meter, exceeds the reserved
/// units, or cannot be committed after the durable start.
pub fn record_metering_receipt<E: EventStore>(
    events: &mut E,
    started: StartedMeteredAction,
    receipt: ExternalMeteringReceipt,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<MeteredActionState, MeteringCoordinatorError> {
    if receipt.meter != started.reservation.meter {
        return invalid("metering receipt uses another meter");
    }
    if receipt.charged_units.get() > started.reservation.reserved_units.get() {
        return invalid("metering receipt exceeds its pre-effect reservation");
    }
    let payload = ReceiptPayload {
        episode_id: started.reservation.episode_id,
        action_id: started.reservation.action_id,
        receipt: receipt.clone(),
    };
    let event = fact(
        RECEIPTED,
        Some(started.started_event_id),
        observed_at,
        &payload,
    )?;
    events.append(
        &started.action_stream,
        ExpectedRevision::Exact(started.revision),
        command_id,
        &[event],
    )?;
    Ok(MeteredActionState::Receipted {
        reservation: started.reservation,
        receipt,
    })
}

/// Recovers one action without granting authority after a durable start.
///
/// # Errors
///
/// Returns [`MeteringCoordinatorError`] when the episode, meter ledger, or action saga contradicts
/// its frozen policy and causal history, or storage cannot be read.
pub fn recover_metered_action<E: EventStore>(
    events: &E,
    episode: &AgentEpisode,
    action_id: MeteredActionId,
) -> Result<MeteredActionState, MeteringCoordinatorError> {
    let ledger = ledger_stream(episode.episode_id())?;
    let ledger_history = events.read_stream(&ledger, None)?;
    let entries = project_ledger(&ledger_history, episode.episode_id())?;
    let snapshot = crate::episode::recover_budget_snapshot(events, episode)
        .map_err(|error| MeteringCoordinatorError::InvalidHistory(error.to_string()))?;
    validate_ledger(&entries, snapshot.budget.external_meter_limits.as_deref())?;
    let Some(entry) = entries
        .iter()
        .find(|entry| entry.payload.action_id == action_id)
    else {
        return Ok(MeteredActionState::NotFound);
    };
    let reservation = reservation_from(&entry.payload);
    if entry.payload.decision == ReservationDecision::Denied {
        return Ok(MeteredActionState::Denied {
            reservation,
            limit: entry.payload.limit,
        });
    }
    let stream = action_stream(action_id)?;
    let history = events.read_stream(&stream, None)?;
    project_action(&history, entry, reservation, stream)
}

pub(crate) fn validate_external_meter_limits(
    limits: Option<&[EpisodeExternalMeterLimit]>,
) -> Result<(), MeteringCoordinatorError> {
    let mut meters = HashSet::new();
    for limit in limits.into_iter().flatten() {
        let meter: String = limit.meter.clone().into();
        if !meters.insert(meter) {
            return invalid("external meter limits contain a duplicate meter");
        }
    }
    Ok(())
}

fn reservation_decision(
    configured: Option<&[EpisodeExternalMeterLimit]>,
    entries: &[ReservationEntry],
    meter: &ExternalMeterName,
    requested: ExternalMeteredUnits,
) -> Result<(ReservationDecision, Option<ExternalMeteredUnits>), MeteringCoordinatorError> {
    let Some(configured) = configured else {
        return Ok((ReservationDecision::Reserved, None));
    };
    let Some(limit) = configured.iter().find(|limit| &limit.meter == meter) else {
        return Ok((ReservationDecision::Denied, None));
    };
    let reserved = entries.iter().try_fold(0_u64, |total, entry| {
        if entry.payload.decision == ReservationDecision::Reserved && &entry.payload.meter == meter
        {
            total
                .checked_add(entry.payload.reserved_units.get())
                .ok_or_else(|| {
                    MeteringCoordinatorError::InvalidHistory(
                        "meter reservation total overflow".into(),
                    )
                })
        } else {
            Ok(total)
        }
    })?;
    let fits = reserved
        .checked_add(requested.get())
        .is_some_and(|total| total <= limit.units.get());
    Ok((
        if fits {
            ReservationDecision::Reserved
        } else {
            ReservationDecision::Denied
        },
        Some(limit.units),
    ))
}

fn outcome_from_entry<E: EventStore>(
    events: &E,
    entry: &ReservationEntry,
    reservation: MeterReservation,
) -> Result<MeterReservationOutcome, MeteringCoordinatorError> {
    match entry.payload.decision {
        ReservationDecision::Denied => Ok(MeterReservationOutcome::Denied {
            reservation,
            limit: entry.payload.limit,
        }),
        ReservationDecision::Reserved => {
            let stream = action_stream(reservation.action_id)?;
            if !events.read_stream(&stream, None)?.is_empty() {
                return invalid("replayed meter reservation authority was already consumed");
            }
            Ok(MeterReservationOutcome::Reserved(MeteredActionAuthority {
                reservation,
                reservation_event_id: entry.event_id,
                action_stream: stream,
            }))
        }
    }
}

fn project_ledger(
    history: &[EventEnvelope],
    episode_id: EpisodeId,
) -> Result<Vec<ReservationEntry>, MeteringCoordinatorError> {
    let mut entries = Vec::new();
    let mut action_ids = HashSet::new();
    let mut parent = None;
    for event in history {
        if event.parent_event_id != parent
            || ![RESERVED, DENIED].contains(&event.schema_name.as_str())
        {
            return invalid("meter ledger causal chain or schema is invalid");
        }
        let payload: ReservationPayload = decode(event)?;
        let expected = if event.schema_name.as_str() == RESERVED {
            ReservationDecision::Reserved
        } else {
            ReservationDecision::Denied
        };
        if payload.episode_id != episode_id || payload.decision != expected {
            return invalid("meter reservation decision contradicts its ledger event");
        }
        if !action_ids.insert(payload.action_id.to_string()) || payload.reserved_units.get() == 0 {
            return invalid(
                "meter ledger reuses an action identity or contains a zero reservation",
            );
        }
        entries.push(ReservationEntry {
            payload,
            event_id: event.event_id,
            command_id: event.command_id,
            observed_at_unix_ms: event.observed_at_unix_ms,
        });
        parent = Some(event.event_id);
    }
    Ok(entries)
}

fn validate_ledger(
    entries: &[ReservationEntry],
    configured: Option<&[EpisodeExternalMeterLimit]>,
) -> Result<(), MeteringCoordinatorError> {
    for (index, entry) in entries.iter().enumerate() {
        let (decision, limit) = reservation_decision(
            configured,
            &entries[..index],
            &entry.payload.meter,
            entry.payload.reserved_units,
        )?;
        if entry.payload.decision != decision || entry.payload.limit != limit {
            return invalid("meter ledger decision contradicts the frozen episode budget");
        }
    }
    Ok(())
}

fn project_action(
    history: &[EventEnvelope],
    entry: &ReservationEntry,
    reservation: MeterReservation,
    stream: StreamId,
) -> Result<MeteredActionState, MeteringCoordinatorError> {
    if history.is_empty() {
        return Ok(MeteredActionState::ReadyToStart(MeteredActionAuthority {
            reservation,
            reservation_event_id: entry.event_id,
            action_stream: stream,
        }));
    }
    if history.len() > 2 || history[0].schema_name.as_str() != STARTED {
        return invalid("metered action does not start with exactly one started fact");
    }
    let started: StartedPayload = decode(&history[0])?;
    if history[0].parent_event_id != Some(entry.event_id)
        || started.episode_id != reservation.episode_id
        || started.action_id != reservation.action_id
        || started.meter != reservation.meter
        || started.reserved_units != reservation.reserved_units
    {
        return invalid("metered-action start differs from its reservation");
    }
    if history.len() == 1 {
        return Ok(MeteredActionState::InDoubt(StartedMeteredAction {
            reservation,
            action_stream: stream,
            revision: revision(history[0].sequence)?,
            started_event_id: history[0].event_id,
        }));
    }
    let receipt_event = &history[1];
    if receipt_event.schema_name.as_str() != RECEIPTED
        || receipt_event.parent_event_id != Some(history[0].event_id)
    {
        return invalid("metering receipt does not cite its started fact");
    }
    let payload: ReceiptPayload = decode(receipt_event)?;
    if payload.episode_id != reservation.episode_id
        || payload.action_id != reservation.action_id
        || payload.receipt.meter != reservation.meter
        || payload.receipt.charged_units.get() > reservation.reserved_units.get()
    {
        return invalid("metering receipt differs from its bounded reservation");
    }
    Ok(MeteredActionState::Receipted {
        reservation,
        receipt: payload.receipt,
    })
}

fn reservation_from(payload: &ReservationPayload) -> MeterReservation {
    MeterReservation {
        episode_id: payload.episode_id,
        action_id: payload.action_id,
        meter: payload.meter.clone(),
        reserved_units: payload.reserved_units,
    }
}

fn ledger_stream(episode_id: EpisodeId) -> Result<StreamId, MeteringCoordinatorError> {
    stream("agent-meter-ledger", episode_id.to_string())
}

fn action_stream(action_id: MeteredActionId) -> Result<StreamId, MeteringCoordinatorError> {
    stream("agent-metered-action", action_id.to_string())
}

fn stream(kind: &str, id: String) -> Result<StreamId, MeteringCoordinatorError> {
    Ok(StreamId {
        kind: AggregateKind::new(kind)
            .map_err(|error| MeteringCoordinatorError::InvalidHistory(error.to_string()))?,
        id: AggregateId::new(id)
            .map_err(|error| MeteringCoordinatorError::InvalidHistory(error.to_string()))?,
    })
}

fn fact<P: Serialize>(
    schema: &str,
    parent_event_id: Option<EventId>,
    observed_at: ObservedAtUnixMillis,
    payload: &P,
) -> Result<NewEvent, MeteringCoordinatorError> {
    Ok(NewEvent {
        schema_name: SchemaName::new(schema)
            .map_err(|error| MeteringCoordinatorError::InvalidHistory(error.to_string()))?,
        schema_version: SchemaVersion::new(1)
            .map_err(|error| MeteringCoordinatorError::InvalidHistory(error.to_string()))?,
        parent_event_id,
        observed_at_unix_ms: observed_at.get(),
        payload: cairn_codec::to_vec(payload)
            .map_err(|error| MeteringCoordinatorError::InvalidHistory(error.to_string()))?,
    })
}

fn decode<P: for<'de> Deserialize<'de>>(
    event: &EventEnvelope,
) -> Result<P, MeteringCoordinatorError> {
    if event.schema_version.get() != 1 {
        return invalid("unsupported metering event schema version");
    }
    cairn_codec::from_slice(&event.payload)
        .map_err(|error| MeteringCoordinatorError::InvalidHistory(error.to_string()))
}

fn revision(
    sequence: cairn_protocol::EventSequence,
) -> Result<StreamRevision, MeteringCoordinatorError> {
    StreamRevision::new(sequence.get())
        .map_err(|error| MeteringCoordinatorError::InvalidHistory(error.to_string()))
}

fn invalid<T>(message: &str) -> Result<T, MeteringCoordinatorError> {
    Err(MeteringCoordinatorError::InvalidHistory(message.to_owned()))
}

#[cfg(test)]
mod tests {
    use cairn_protocol::{CommandId, EpisodeId, MeteredActionId, ModelAttemptId, StepId, TaskId};
    use cairn_store_sqlite::SqliteEventStore;

    use super::{
        EpisodeExternalMeterLimit, ExternalMeteredUnits, ExternalMeteringReceipt,
        MeterReservationOutcome, MeteredActionState, ReservationDecision, ReservationEntry,
        ReservationPayload, begin_metered_action, record_metering_receipt, recover_metered_action,
        reserve_metered_action, validate_ledger,
    };
    use crate::{
        AgentEpisode, AgentRoleName, EpisodeBudget, ExternalMeterName, ExternalReceiptReference,
        open_agent_episode,
    };

    fn meter(name: &str) -> ExternalMeterName {
        ExternalMeterName::new(name).expect("meter")
    }

    fn opened_episode(
        events: &mut SqliteEventStore,
        limits: Option<Vec<EpisodeExternalMeterLimit>>,
    ) -> AgentEpisode {
        let episode = AgentEpisode::new(EpisodeId::new()).expect("episode");
        open_agent_episode(
            events,
            &episode,
            TaskId::new(),
            AgentRoleName::new("candidate-author").expect("role"),
            EpisodeBudget {
                step_limit: None,
                tool_operation_limit: None,
                provider_token_limit: None,
                deadline_unix_ms: None,
                external_meter_limits: limits,
            },
            StepId::new(),
            ModelAttemptId::new(),
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(1),
        )
        .expect("open episode");
        episode
    }

    fn reserve(
        events: &mut SqliteEventStore,
        episode: &AgentEpisode,
        action_id: MeteredActionId,
        meter: &str,
        units: u64,
        command_id: &CommandId,
        observed_at: i64,
    ) -> MeterReservationOutcome {
        reserve_metered_action(
            events,
            episode,
            action_id,
            self::meter(meter),
            ExternalMeteredUnits::new(units),
            command_id,
            cairn_protocol::ObservedAtUnixMillis::new(observed_at),
        )
        .expect("reservation decision")
    }

    #[test]
    fn disabled_meter_budget_records_but_does_not_restrict_reservations() {
        let mut events = SqliteEventStore::in_memory().expect("events");
        let episode = opened_episode(&mut events, None);
        let action_id = MeteredActionId::new();
        let command_id = CommandId::new();
        let MeterReservationOutcome::Reserved(lost_authority) = reserve(
            &mut events,
            &episode,
            action_id,
            "usd-micros",
            u64::MAX,
            &command_id,
            2,
        ) else {
            panic!("disabled budget must reserve");
        };
        assert_eq!(
            lost_authority.reservation().reserved_units().get(),
            u64::MAX
        );
        drop(lost_authority);

        let MeterReservationOutcome::Reserved(recovered_authority) = reserve(
            &mut events,
            &episode,
            action_id,
            "usd-micros",
            u64::MAX,
            &command_id,
            2,
        ) else {
            panic!("lost reservation acknowledgement must replay");
        };
        assert_eq!(recovered_authority.reservation().action_id(), action_id);
        assert!(matches!(
            reserve(
                &mut events,
                &episode,
                MeteredActionId::new(),
                "gpu-millis",
                9,
                &CommandId::new(),
                3,
            ),
            MeterReservationOutcome::Reserved(_)
        ));
    }

    #[test]
    fn enabled_meter_budget_is_independent_per_meter_and_fails_closed() {
        let mut events = SqliteEventStore::in_memory().expect("events");
        let episode = opened_episode(
            &mut events,
            Some(vec![
                EpisodeExternalMeterLimit::new(meter("usd-micros"), ExternalMeteredUnits::new(10)),
                EpisodeExternalMeterLimit::new(meter("gpu-millis"), ExternalMeteredUnits::new(2)),
            ]),
        );
        assert!(matches!(
            reserve(
                &mut events,
                &episode,
                MeteredActionId::new(),
                "usd-micros",
                6,
                &CommandId::new(),
                2,
            ),
            MeterReservationOutcome::Reserved(_)
        ));
        let denied_id = MeteredActionId::new();
        let denied_command = CommandId::new();
        let MeterReservationOutcome::Denied { limit, .. } = reserve(
            &mut events,
            &episode,
            denied_id,
            "usd-micros",
            5,
            &denied_command,
            3,
        ) else {
            panic!("reservation must exceed the configured meter");
        };
        assert_eq!(limit.map(ExternalMeteredUnits::get), Some(10));
        assert!(matches!(
            reserve(
                &mut events,
                &episode,
                denied_id,
                "usd-micros",
                5,
                &denied_command,
                3,
            ),
            MeterReservationOutcome::Denied { .. }
        ));
        assert!(matches!(
            reserve(
                &mut events,
                &episode,
                MeteredActionId::new(),
                "network-bytes",
                1,
                &CommandId::new(),
                4,
            ),
            MeterReservationOutcome::Denied { limit: None, .. }
        ));
        assert!(matches!(
            reserve(
                &mut events,
                &episode,
                MeteredActionId::new(),
                "gpu-millis",
                2,
                &CommandId::new(),
                5,
            ),
            MeterReservationOutcome::Reserved(_)
        ));
    }

    #[test]
    fn started_action_recovers_in_doubt_and_accepts_only_a_bounded_receipt() {
        let mut events = SqliteEventStore::in_memory().expect("events");
        let episode = opened_episode(
            &mut events,
            Some(vec![EpisodeExternalMeterLimit::new(
                meter("usd-micros"),
                ExternalMeteredUnits::new(20),
            )]),
        );
        let action_id = MeteredActionId::new();
        let reservation_command = CommandId::new();
        let MeterReservationOutcome::Reserved(authority) = reserve(
            &mut events,
            &episode,
            action_id,
            "usd-micros",
            8,
            &reservation_command,
            2,
        ) else {
            panic!("reserved");
        };
        assert!(matches!(
            recover_metered_action(&events, &episode, action_id).expect("recover ready"),
            MeteredActionState::ReadyToStart(_)
        ));
        let started = begin_metered_action(
            &mut events,
            authority,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(3),
        )
        .expect("durable start");
        drop(started);
        assert!(
            reserve_metered_action(
                &mut events,
                &episode,
                action_id,
                meter("usd-micros"),
                ExternalMeteredUnits::new(8),
                &reservation_command,
                cairn_protocol::ObservedAtUnixMillis::new(2),
            )
            .is_err(),
            "a durable start consumes execution authority"
        );
        let MeteredActionState::InDoubt(started) =
            recover_metered_action(&events, &episode, action_id).expect("recover in doubt")
        else {
            panic!("started action must be in doubt");
        };
        assert_eq!(started.reservation().action_id(), action_id);
        let receipt = ExternalMeteringReceipt::new(
            meter("usd-micros"),
            ExternalMeteredUnits::new(7),
            Some(ExternalReceiptReference::new("invoice-7").expect("reference")),
        );
        assert!(matches!(
            record_metering_receipt(
                &mut events,
                started,
                receipt.clone(),
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(4),
            )
            .expect("record receipt"),
            MeteredActionState::Receipted { .. }
        ));
        let MeteredActionState::Receipted {
            reservation,
            receipt: recovered,
        } = recover_metered_action(&events, &episode, action_id).expect("recover receipt")
        else {
            panic!("receipt must be durable");
        };
        assert_eq!(reservation.reserved_units().get(), 8);
        assert_eq!(recovered, receipt);
    }

    #[test]
    fn invalid_receipt_does_not_remove_reconciliation_capability() {
        let mut events = SqliteEventStore::in_memory().expect("events");
        let episode = opened_episode(&mut events, None);
        let action_id = MeteredActionId::new();
        let MeterReservationOutcome::Reserved(authority) = reserve(
            &mut events,
            &episode,
            action_id,
            "credits",
            5,
            &CommandId::new(),
            2,
        ) else {
            panic!("reserved");
        };
        let started = begin_metered_action(
            &mut events,
            authority,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(3),
        )
        .expect("started");
        assert!(
            record_metering_receipt(
                &mut events,
                started,
                ExternalMeteringReceipt::new(meter("other"), ExternalMeteredUnits::new(1), None,),
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(4),
            )
            .is_err()
        );
        let MeteredActionState::InDoubt(started) =
            recover_metered_action(&events, &episode, action_id).expect("recover after rejection")
        else {
            panic!("invalid receipt must not alter durable state");
        };
        assert!(
            record_metering_receipt(
                &mut events,
                started,
                ExternalMeteringReceipt::new(meter("credits"), ExternalMeteredUnits::new(6), None,),
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(5),
            )
            .is_err()
        );
        assert!(matches!(
            recover_metered_action(&events, &episode, action_id).expect("still in doubt"),
            MeteredActionState::InDoubt(_)
        ));
    }

    #[test]
    fn duplicate_configuration_and_forged_ledger_decisions_fail_closed() {
        let duplicate = vec![
            EpisodeExternalMeterLimit::new(meter("credits"), ExternalMeteredUnits::new(4)),
            EpisodeExternalMeterLimit::new(meter("credits"), ExternalMeteredUnits::new(8)),
        ];
        let mut events = SqliteEventStore::in_memory().expect("events");
        let episode = AgentEpisode::new(EpisodeId::new()).expect("episode");
        assert!(
            open_agent_episode(
                &mut events,
                &episode,
                TaskId::new(),
                AgentRoleName::new("candidate-author").expect("role"),
                EpisodeBudget {
                    step_limit: None,
                    tool_operation_limit: None,
                    provider_token_limit: None,
                    deadline_unix_ms: None,
                    external_meter_limits: Some(duplicate),
                },
                StepId::new(),
                ModelAttemptId::new(),
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(1),
            )
            .is_err()
        );

        let forged = ReservationEntry {
            payload: ReservationPayload {
                episode_id: EpisodeId::new(),
                action_id: MeteredActionId::new(),
                meter: meter("credits"),
                reserved_units: ExternalMeteredUnits::new(1),
                decision: ReservationDecision::Reserved,
                limit: None,
            },
            event_id: cairn_protocol::EventId::derive(b"forged").expect("event id"),
            command_id: CommandId::new(),
            observed_at_unix_ms: 1,
        };
        assert!(validate_ledger(&[forged], Some(&[])).is_err());
    }
}
