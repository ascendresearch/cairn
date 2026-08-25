# Scheduler and controller composition

The scheduler kernel is domain-neutral. It receives an already frozen
`JobContract`/`PlacementRequest`, a controller-supplied set of stable `WorkerId` candidates, current
worker facts, and a trusted credential-authority adapter. It does not know migration stages, oracle
roles, agent roles, or model providers.

## Identity and authority chain

```text
AttemptId
  -> PlacementId + PlacementSnapshotArtifact
  -> ReservationId
  -> AssignmentId + LeaseId
```

`PlacementId` names one immutable evaluation. Its content-addressed snapshot contains the policy,
observation time, exact candidate evidence, every rejection, and the selected worker if one exists.
`ReservationId` is capacity authority, not an alias for the choice or lease. It is committed before
the assignment layer can create delivery authority.

The supported policy is `stable-worker-id-quantitative-v2`: candidates are canonicalized by
`WorkerId`, pass pool/platform/backend/capability, liveness, availability, controller credential
authority, and slot/quantitative reservation-capacity gates, then the first eligible worker is
selected. A no-candidate result is also durable and retains the complete rejection trace.

## Capacity and concurrency

The V2 reservation ledger is one global event stream. For each worker, effective slot admission
is bounded by registered maximum concurrency minus unreleased reservations. Dynamic availability
also subtracts reservations whose `AttemptId` is not yet present in the worker heartbeat; attempts
already reported active are not double-subtracted. SQLite expected-revision concurrency means
competing controller writers cannot both commit against the same free slot. One `AttemptId` cannot
hold parallel active reservations. A revision loser may retry with a fresh `PlacementId`; it cannot
reinterpret the losing snapshot as authority.

Each placement also freezes the current typed resource-observation ContentId, worker-stream event
revision, and optional trusted-admission evidence. Its reservation records the requested CPU,
memory, and scratch quantities plus exact accelerator device IDs. All unreleased quantities are
subtracted before a new candidate becomes eligible. Devices are selected deterministically from the
canonical matching set and are exclusive; disappearance of an actively reserved device blocks new
quantitative placement until safe release. The same global optimistic revision protects slot and
quantitative accounting, including concurrent writers.

This singleton design favors an auditable correctness baseline over horizontal write throughput.
A later sharded ledger must retain the same `PlacementId`, `ReservationId`, snapshot, and release
proof semantics.

## Staleness and release

Before assignment lease grant, the scheduler reloads the selected worker and requires the same
incarnation, credential, profile, resource ContentId/revision/admission evidence, availability
artifact, and heartbeat observation. It separately rechecks controller-owned credential authority.
Any change—including a resource refresh that reports more capacity—fails closed and leaves the
bounded reservation for recovery.

Reservation claim timing is an explicit positive `ReservationClaimTimeoutMillis`; it is not a
hard-coded budget. An unclaimed reservation may be released after that deadline only if no
assignment stream exists. A claimed reservation requires recovered proof of terminal execution or
lease expiry before execution start. Running and reconciliation-required/in-doubt states never
release capacity automatically.

## Controller composition and retry

`cairn-server` now composes contract preparation, placement reservation, conditional attempt
authorization, assignment grant, and assignment-offer enqueueing. A no-candidate placement does not
invent execution authority. Product orchestration allocates the complete strongly typed identity
set before invoking this boundary and retains it until the outcome is durable. An exact retry reuses
the same attempt, placement, reservation, assignment, lease, logical message, and command
identities. The controller recovers an existing assignment phase rather than granting another
lease; a still-leased offer is safely re-enqueued if response or acknowledgement ordering made that
necessary.

When the authenticated worker durably accepts an offer, the controller commits authoritative
execution start before enqueueing the stable `StartExecution` message. A crash after acceptance is
recovered from the accepted assignment; a crash after the start fact but before outbox enqueue is
recovered from running state and reuses the frozen start-message identity. The live two-worker
control carries one assignment through offer, acceptance, start, the worker's conservative
`NotStarted` result, terminal reconciliation, exact scheduling retry, and reservation release.

The candidate universe is the canonical union of managed-registry and transitional static worker
identities. Every managed credential observation reopens and projects the SQLite enrollment stream,
then cites its latest `EventId`. Consequently the assignment-grant recheck cannot reuse the
placement-time registry view: revocation, worker disablement, rotation retirement, or authority
unavailability between snapshot and grant fails closed. Static credentials cite no registry event
until Phase E imports them into managed history.

`scheduler` is an optional controller configuration object. Omitting it or setting it to `null`
disables new placement without disabling worker control or reconciliation. When enabled, the policy
version, reservation-claim timeout, and assignment-lease duration are explicit; session liveness
uses the independently configured controller session timeout. All enabled durations are positive
strong types and zero is rejected during configuration decoding.

## Migration translation boundary

`cairn-migration` owns `MigrationValidationTier` and `MigrationExecutionNeed`. Translation emits
only a generic backend and `ResourceRequest` over platform, authenticated pools, capabilities, and
timeout. The product tier remains above the boundary and never enters `PlacementRequest`, worker
profiles, assignment bindings, or scheduler snapshots. The hardware-free integration fixture runs
a V3 migration need through this translation, controller composition, SQLite reservation, lease,
and durable worker outbox.
