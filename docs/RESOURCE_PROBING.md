# Worker resource probing

- Status: Phase D1 implemented; Phase D2 explicitly deferred
- Date: 2026-08-25
- Scope: Linux startup observation, operator configuration, matching, and trust boundaries

## What D1 guarantees

Each worker incarnation constructs profile V3 from two different kinds of input:

- observed facts: native platform, logical CPU count, total memory bytes, available bytes on the
  configured scratch filesystem, accelerator discovery completeness, and canonical device facts;
- operator declarations: supported execution backends and worker-level equality capabilities.

The quantitative observation records `BuiltinProbe` provenance, a probe-version label,
`observed_at`, and an optional exclusive `valid_until`. It is archived inside the immutable worker
profile. Job contract V3 can request independent minimum CPU, memory, and scratch quantities plus
an accelerator request. An accelerator request is a positive minimum device count and a canonical
set of per-device equality capabilities. Only devices satisfying every requested capability count.

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

`config/worker.example.json` is the strict schema V4 example. `resource_probe` contains:

- `scratch_path`: required path whose available filesystem bytes are observed;
- `accelerator_sysfs`: path to inspect, or `null` to disable discovery explicitly;
- `freshness_ms`: positive lifetime, or `null` for no expiry during this incarnation;
- `expected`: independently optional minima plus a completeness assertion.

Expected values never overwrite probe results. They are startup assertions. Relative probe paths
are resolved relative to the worker configuration file just like journal and identity paths.

Because D1 observes only at process startup, setting `freshness_ms` makes the worker ineligible when
that exclusive deadline arrives. Use `null` when incarnation-scoped evidence is sufficient. D2 will
add refresh without requiring process restart.

## Scheduler behavior and current limit

Candidate filtering and assignment grant evaluate resource freshness at their own controller
observation time. They reject insufficient CPU, memory, scratch, discovery completeness, matching
accelerator count, or ordinary platform/backend/capability constraints before availability.

D1 does not subtract quantitative amounts for already-live reservations. The durable scheduler
continues to prevent overcommit by assignment slots, and each quantitative request must fit the
startup total, but two concurrent requests can each match the same CPU or memory total. Phase D2
will introduce a separately versioned resource-observation stream, reservation accounting per
dimension, and exact observation-revision recheck.

## Provenance and admission

A worker hello may assert only `BuiltinProbe` provenance for platform and quantitative observation,
and `OperatorDeclared` provenance for backend/worker capabilities. It cannot self-label a claim as
`ControllerVerified` or `ExternalAttestation`.

Phase D2 will add typed admission facts that can supersede a built-in observation after a controller
challenge or external attestation. This will be an independent authority stream, not mutation of a
historical profile.
