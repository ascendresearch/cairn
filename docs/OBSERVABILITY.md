# Operational observability and logging

- Status: normative logging contract and active coverage audit
- Date: 2026-08-26
- Decision: `D-023`
- Requirements: `QR-OBS-001..008`, `QR-SEC-004`, `FR-REC-*`

## 1. Boundary

Cairn has two different observation systems:

1. durable events and typed content reconstruct authority, external-effect state, evidence, and
   verdicts;
2. structured logs help a human or collector see a running process, correlate work, and diagnose
   delay or failure.

Logs are never replay input. Dropping or filtering every log must leave the same durable facts and
content identities. Conversely, a log line cannot prove that a model request, tool effect,
assignment, execution, or verdict committed; the cited durable identity must be inspected.

This is a source-code ownership rule as well as a runtime rule. Logging events may observe only
immutable typed identities, already-computed classifications, and infallible bounded scalar
projections. They must not execute or retry a capability, create an identity, obtain authoritative
time, classify an outcome a second time, mutate state, await work, or propagate `?`. Cairn V1 uses
no tracing spans or `instrument` in business crates: business work cannot live inside a logging
scope whose later removal could remove the work. `scripts/check-log-isolation.sh` enforces the
mechanically detectable part of this rule in CI.

## 2. Audit finding

Before this slice, the workspace had no logging subscriber and no shared logging dependency.
Server and worker contained only a small number of free-form `eprintln!` calls. Generic model
dispatch, tools, episode budgets, scheduling, executor invocation, and Oracle revision were silent.
The live Oracle gate therefore appeared idle between its initial and final aggregate even though
the provider console showed sequential requests.

Implemented coverage now is:

| Boundary | Default events | Correlation |
|---|---|---|
| process | logging initialized, listener ready, process/subsystem failure | component, target |
| model provider | dispatch started/completed/failed, token/cache usage, elapsed time | attempt, request, decision, response IDs |
| tool gateway | operation started/completed/failed, effect and elapsed time | operation, attempt, argument/result IDs |
| agent episode | opened/completed and typed budget terminal reason | task, episode, step, model-attempt IDs |
| Optional model debate dogfood | research, submission repair, adversarial blocker, synthesis revision, recheck, convergence | sample and frozen draft/review IDs |
| scheduler | start, optimistic retry, no candidate, placement completion | attempt, placement, snapshot, reservation, assignment, worker IDs |
| worker session | connect/register/disconnect/error | worker, incarnation, connection, pool |
| assignment/execution | offer/admission/start/result and local/controller terminal receipt | job, assignment, attempt, contract, receipt IDs |
| Controller workflow / Oracle control | durable transition committed; qualified control prepare/start/block/complete | task, command, event, run, runner, job, attempt, contract, receipt IDs |
| local App API | listener ready; submit/cancel/intent decision committed; request/supervisor rejected | task, command, request, hypothesis IDs; response kind and bounded counts |
| heartbeat/resources | sent/accepted | worker and connection IDs; DEBUG only |
| registry/enrollment | mutation and listener/bundle lifecycle without secret path or bytes | event, worker, credential/reservation IDs where applicable |

Still incomplete: candidate/admission comparison phases need their own lifecycle events; the local
App API has lifecycle logging but no distributed trace propagation; metrics export, dashboards,
and alert rules are not implemented. SQLite/CAS reads and frame acknowledgements deliberately are
not logged at INFO because per-operation lifecycle and durable events provide the useful boundary
without flooding.

## 3. Initialization and output

Every runnable server, worker, and live model gate initializes `cairn-observability` once. Logs go
to stderr so strict JSON command results remain isolated on stdout.

Environment variables:

```text
CAIRN_LOG=info
CAIRN_LOG_FORMAT=json
```

`CAIRN_LOG` accepts `EnvFilter` directives. Examples:

```bash
CAIRN_LOG=debug CAIRN_LOG_FORMAT=compact target/debug/cairn-worker config.json
CAIRN_LOG='info,cairn.agent.model=debug,cairn.oracle.debate=debug' \
  cargo run -p cairn-migration --example oracle_model_debate_live -- CONFIG SAMPLE
```

The default is JSON at INFO. `compact` is intended for an interactive terminal. ANSI is disabled
in both modes. Invalid filter or format fails startup. Cairn writes no rotating log files; systemd,
container runtime, or another collector owns routing, retention, access control, and rotation.

## 4. Event and field contract

Every event has the subscriber-provided timestamp, level, target, thread identity, event name, and
message. Work events add the strongest identities already available at that boundary:

- `task_id`, `episode_id`, `step_id`;
- `attempt_id`, `operation_id`, `job_id`;
- `placement_id`, `snapshot_id`, `reservation_id`, `assignment_id`;
- `worker_id`, `incarnation_id`, `connection_id`;
- `request_id`, `decision_id`, `response_id`, `contract_id`, `result_id`, `receipt_id`.

Start/terminal pairs use `_started`, `_completed`, `_failed`, or a precise state name. Terminal
events add classification, `elapsed_ms`, counts, and provider usage when supplied. Absence remains
absent rather than zero. Content IDs correlate safe metadata without printing content bytes.

Level policy:

- ERROR: a process or independently spawned subsystem terminates;
- WARN: rejection, ambiguity, no-candidate, retry, blocker, or recoverable session failure;
- INFO: startup and meaningful work lifecycle boundaries;
- DEBUG: heartbeat, resource refresh, HTTP/wire-adjacent bounded metadata;
- TRACE: currently unused; enabling it must not expose forbidden content.

## 5. Sensitive-data policy

Never log:

- credentials, bearer/API headers, private keys, certificate or enrollment-bundle bytes;
- prompt, native request, raw provider response, reasoning, model final text;
- tool arguments or results, upstream/source blobs, input bundles;
- workload stdout/stderr or declared output bytes;
- arbitrary opaque diagnostics at generic provider/tool/executor boundaries.

Safe fields include typed identities, registered names, protocol/effect/outcome classifications,
HTTP-independent byte counts, token/cache counts, exit code, elapsed time, and number of outputs.
The exact diagnostic remains in its bounded durable fact. A log states `diagnostic_archived=true`
and provides the correlated attempt or operation identity.

The model-dispatch test installs an isolated JSON subscriber around secret-sentinel request and
response bytes. It asserts lifecycle/usage/attempt fields are present and both sentinels are absent.
This complements, rather than replaces, credential and export secret scans.

## 6. Operator queries

Typical JSON-log queries are intentionally based on stable event names and identities:

```bash
jq 'select(.fields.event == "model_dispatch_completed")' service.log
jq 'select(.fields.attempt_id == "attempt:...")' service.log
jq 'select(.level == "WARN" or .level == "ERROR")' service.log
```

A timeline assembled from logs is diagnostic only. The corresponding event store and typed content
must be used for exact reconstruction, reconciliation, or admission evidence.
