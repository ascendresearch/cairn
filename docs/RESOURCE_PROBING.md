# Worker resource probing

- Status: Phase D1 and D2 core implemented
- Date: 2026-08-25
- Scope: Linux startup observation, operator configuration, matching, and trust boundaries

## Observation model

Each worker incarnation constructs profile V1 from two different kinds of input:

- observed facts: native platform, logical CPU count, total memory bytes, available bytes on the
  configured scratch filesystem, accelerator discovery completeness, and canonical device facts;
- operator declarations: supported execution backends and worker-level equality capabilities.

The startup quantitative observation remains archived inside immutable profile V1, but the current
observation is a separate `execution.worker-resource-observation.v1` content identity. Each
registration or `execution.worker-resources-observed` fact freezes that ContentId, its exact worker-
stream event revision, and optional trusted-admission evidence. A refresh therefore never mutates
profile identity or extends heartbeat liveness.

Job contract V1 can request independent minimum CPU, memory, and scratch quantities plus an
accelerator request. An accelerator request is a positive minimum device count and a canonical set
of per-device equality capabilities. Only devices satisfying every requested capability count.

All quantities have unit-specific positive types. CPU count, memory bytes, scratch bytes, and
accelerator count are not interchangeable `u64` values in Rust even though their canonical JSON
wire representation is an integer.

## Linux probe semantics

The initial probe reads:

| Fact | Source | Meaning |
|---|---|---|
| logical CPUs | `std::thread::available_parallelism` | concurrency visible to the process |
| memory | `/proc/meminfo` `MemTotal` | kernel-reported total, strictly parsed from `kB` to bytes |
| scratch | `statvfs(scratch_path)` | available fragment count multiplied by fragment size |
| accelerators | entries under `accelerator_sysfs` | generic device IDs plus selected `uevent` facts |

The generic uevent adapter recognizes `DRIVER`, `PCI_ID`, and `MODALIAS`; it does not claim that
`/sys/class/accel` covers every GPU/NPU vendor. A later vendor adapter can emit the same domain
types without changing scheduling semantics.

Accelerator discovery has deliberate three-way behavior:

- configured namespace exists and every entry is readable: `complete`, including a valid empty set;
- configured namespace does not exist: `complete` empty for that named namespace;
- discovery is disabled with `null`, or any entry cannot be inspected: `partial`.

Requests that require complete discovery reject a partial observation. Invalid `MemTotal` units,
arithmetic overflow, non-UTF-8 or duplicate device identities, duplicate device capabilities,
future/expired timestamps, and configured expectation mismatch fail closed.

## Configuration contract

`config/worker.example.json` is the strict schema V1 example. `resource_probe` contains:

- `scratch_path`: required path whose available filesystem bytes are observed;
- `accelerator_sysfs`: path to inspect, or `null` to disable discovery explicitly;
- `freshness_ms`: positive lifetime, or `null` for no expiry during this incarnation;
- `refresh_interval_ms`: positive automatic refresh interval, or `null` to disable refresh;
- `expected`: independently optional minima plus a completeness assertion.

Expected values never overwrite probe results. They are startup assertions. Relative probe paths
are resolved relative to the worker configuration file just like journal and identity paths.

When both refresh and expiry are enabled, `refresh_interval_ms` must be shorter than
`freshness_ms`; an equal or longer interval fails startup. A reconnect probes again before hello,
so an expired historical startup observation does not prevent recovery when current evidence is
available. Setting refresh to `null` is explicit: finite evidence then becomes ineligible at its
exclusive deadline, while `freshness_ms: null` remains valid for the incarnation.

## Scheduler reservation behavior

Candidate filtering and assignment grant evaluate resource freshness at their own controller
observation time. They reject insufficient CPU, memory, scratch, discovery completeness, matching
accelerator count, or ordinary platform/backend/capability constraints before availability.

Placement snapshot V1 freezes the current observation ContentId, its worker event revision, and any
admission evidence. Each durable reservation records its requested CPU, memory, and scratch amounts
and the exact canonical accelerator device IDs selected for it. New placements subtract every
unreleased reservation. Accelerator devices are exclusive; if an active device disappears from a
new observation, Cairn admits no further quantitative reservation on that worker until the old
reservation is safely released.

The singleton SQLite scheduler ledger and optimistic revision check serialize competing placements.
This prevents both slot and quantitative overcommit. Assignment grant reloads the worker and rejects
any changed observation ContentId, event revision, admission evidence, availability, profile,
credential, or incarnation. A refresh between placement and grant therefore fails closed; the
unclaimed reservation remains protected until its configured claim deadline permits safe release.

## Provenance and admission

A worker hello may assert only `BuiltinProbe` provenance for platform and quantitative observation,
and `OperatorDeclared` provenance for backend/worker capabilities. It cannot self-label a claim as
`ControllerVerified` or `ExternalAttestation`.

The trusted admission port accepts only a `ControllerVerified` or `ExternalAttestation` observation
paired with an independently supplied evidence `EventId`. Source/admission mismatch fails closed,
and recovery requires trusted sources to retain that evidence citation. The worker wire path owns no
such capability and accepts only `BuiltinProbe` refreshes. Concrete challenge and attestation
adapters remain future integrations; if automated, their cadence and thresholds must be explicit
configuration rather than constants.
