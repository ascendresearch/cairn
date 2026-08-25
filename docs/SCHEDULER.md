# Scheduler reservation kernel

The C1 scheduler is domain-neutral. It receives an already frozen `JobContract`/`PlacementRequest`,
a controller-supplied set of stable `WorkerId` candidates, current worker facts, and a trusted
credential-authority adapter. It does not know migration stages, oracle roles, agent roles, or model
providers.

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

The supported C1 policy is `stable-worker-id-v1`: candidates are canonicalized by `WorkerId`, pass
pool/platform/backend/capability, liveness, availability, controller credential authority, and
reservation-capacity gates, then the first eligible worker is selected. A no-candidate result is
also durable and retains the complete rejection trace.

## Capacity and concurrency

The V1 reservation ledger is one global event stream. For each worker, effective one-slot admission
is bounded by registered maximum concurrency minus unreleased reservations. Dynamic availability
also subtracts reservations whose `AttemptId` is not yet present in the worker heartbeat; attempts
already reported active are not double-subtracted. SQLite expected-revision concurrency means
competing controller writers cannot both commit against the same free slot. One `AttemptId` cannot
hold parallel active reservations. A revision loser may retry with a fresh `PlacementId`; it cannot
reinterpret the losing snapshot as authority.

This singleton design favors an auditable correctness baseline over horizontal write throughput.
A later sharded ledger must retain the same `PlacementId`, `ReservationId`, snapshot, and release
proof semantics.

## Staleness and release

Before assignment lease grant, the scheduler reloads the selected worker and requires the same
incarnation, credential, profile, availability artifact, and heartbeat observation. It separately
rechecks controller-owned credential authority. Any change fails closed and leaves the bounded
reservation for recovery.

Reservation claim timing is an explicit positive `ReservationClaimTimeoutMillis`; it is not a
hard-coded budget. An unclaimed reservation may be released after that deadline only if no
assignment stream exists. A claimed reservation requires recovered proof of terminal execution or
lease expiry before execution start. Running and reconciliation-required/in-doubt states never
release capacity automatically.

The controller/enrollment adapter and `cairn-migration` request translation are C2. C1 exposes the
typed seams and durable kernel they must compose; it does not add business vocabulary to worker
profiles.
