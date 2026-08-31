# DEV-035 Cairn SDK、App API 与 reference CLI

- Status: InProgress
- Date: 2026-08-31
- Scope: CUDA task intake through Oracle Admission; Candidate remains unreachable from the public runner
- Requirements: FR-API-001..005, FR-REC-001..015, QR-OBS-001..008

## Objective

Replace direct library/example dogfood with one production client path:

```text
cairn-cli / future TUI / upstream client
→ cairn-sdk typed command and resource contract
→ cairn-server App API
→ task-owned Controller aggregate
→ durable product progress and explicit SIR review
```

The first end-to-end consumer is an operator submitting an arbitrary bounded CUDA task bundle,
reviewing the exact SIR proposal, confirming or supplementing intent, and observing the workflow
through Oracle Admission. Oracle acceptance is the current public terminal; Candidate proposal,
build, and admission are not selected by this runner.

## Decision

1. Add `cairn-sdk` as the owner of strict current-V1 task commands, product resources, progress
   cursor/pages, SIR review requests, and a transport-neutral client port.
2. Add `cairn-cli` as the reference client. Its initial commands are `task submit`, `list`,
   `status`, `watch`, `cancel`, `intent-review`, `intent-select`, `intent-keep-unknown`, and
   `intent-provide`.
3. Keep management commands separate from reconnectable progress queries. A submitted project is
   an ordered bounded file bundle plus typed caller declaration; it never names a server-local
   task path.
4. Implement the first App API transport as bounded canonical V1 frames over a separately
   configured Unix domain socket. Filesystem permissions are the local authorization boundary.
   Remote mTLS/WebSocket or gRPC remains a later adapter over the same SDK contract; no public
   compatibility baseline or protocol negotiation is introduced.
5. Translate Controller history into product lifecycle items. The API never exposes internal event
   payload enums, restricted Admission content, raw prompts/responses, reasoning, source bytes, or
   process diagnostics.
6. Use `CommandId` for idempotent mutations and `EventSequence` for reconnect cursors. A missed
   ephemeral notification cannot remove a durable task fact.
7. Cancellation is a Controller aggregate transition, not process killing by the CLI. External
   effects already authorized before cancellation retain their durable reconciliation duties.
8. Replace the remaining collection-only Intent authority with a task-generic strongly typed
   operation-semantics claim before live `cuda-samples` promotion. No compatibility variant or
   fixture-specific claim is retained.
9. The client never constructs an Admission grant, authenticated subject, or admitted claim for a
   proposal option. It sends the exact request, selected hypothesis, and caller-claim scope; the
   App Server supplies its configured local principal and independent Admission mechanically joins
   the selected resolution with the exact scoped caller declarations. A supplied replacement claim
   is a distinct strong type and follows the same Admission boundary.
10. D-044/DEV-036 removes the independent proposal process. The SDK/App API contract remains the
    normal entry, while SIR/Oracle/Candidate proposal steps execute inside the Controller workflow
    and external capabilities route only through managed Workers.

## Implemented current-V1 path

- `cairn-sdk` owns bounded canonical App API frames, strong mutation/query resources, reconnectable
  progress, and exact SIR review resources. Individual user-decision requests include their typed
  content identities.
- `cairn-cli` recursively freezes a sorted UTF-8 regular-file bundle, rejects symlinks, submits over
  the same Unix socket used by normal execution, and supports query/cancel/watch plus all three SIR
  responses. It has no fixture runner or server-local task-path shortcut.
- `cairn-server` reconstructs tasks from Controller streams, not a second task table; accepts
  caller-generated `TaskId`/`CommandId`; archives exact SIR task artifacts; supervises the durable
  prefix; and prevents this product runner from crossing Oracle acceptance into Candidate.
- the App Server's configured `TaskIntentAuthoritySubject` is the local authentication result.
  Client payloads cannot self-assert it. Selection carries no raw grant or authoritative claim.
- Intent Admission now publishes both the exact scoped caller declarations and the selected or
  supplied conflict resolution. This closes the dogfood-discovered loss of independent invariants
  such as output length when a numerical hypothesis is selected.
- structured logs cover listener readiness, submit/cancel/intent decisions, supervisor failures,
  model-resolution metadata, and existing Controller transitions. They contain correlation IDs,
  classifications and counts only—not source, prompt, response, supplied semantic text, secrets,
  or process output.

## Superseded live dogfood evidence

The normal CLI submitted NVIDIA `cuda-samples` `vectorAdd` as five source/project files. A real
DeepSeek SIR episode produced a source-cited proposal after bounded reads. Restart recovery reused
the exact persisted proposal; deterministic triage produced one exact question instead of rerunning
the model:

- task `task:01a05633-7aca-7ef1-82d7-a07f39b0c245`, final old-V1 sequence `12`;
- proposal `c2182d…9715`, request batch `67e3db…1653`, individual request `8173fc…9275`;
- question: whether signed-zero, NaN and infinity behavior of `A+B+0.0f` is protected;
- offered resolutions: exact CUDA binary32 expression semantics, or the sample harness's absolute
  `1e-5` tolerance without exact special-value semantics.

The first triage implementation required a `desired-semantics` unknown and rejected this otherwise
valid proposal. The current generic closure instead accepts any unknown mechanically joined to a
conflict by an experiment targeting either that conflict directly or at least two of its competing
hypotheses. The generic SIR prompt now states that closure requirement without naming this sample.
The user selected `hyp-exact-ieee-expression`; independent Intent Admission preserved both caller
claims and recorded the admitted intent at sequence 9. Oracle Exploration opened, but the old
process-bound workspace froze a superseded executable identity and stopped before a strategy could
publish. D-044 deletes that process model, so this development database is discarded rather than
decoded, migrated, retried or counted as current architecture evidence.

Remaining before this record can become `Accepted`: finish DEV-036, connect a real local managed
Worker and qualified Oracle mechanism runner to the App Server, resubmit vectorAdd through the CLI,
repeat the user decision under the new task authority, reach Oracle accepted, then repeat the same
path on `cuda-samples` reduction.

## AlloyPort reference disposition

The predecessor's `migrate/runs/status/cancel/attach`, stable retry identity, sorted project bundle,
management/interaction split, and `after_sequence` reconnect semantics inform the client
experience. Cairn does not copy its generic string identities, integer lifecycle states, raw
internal-event subscription, reducer-violation continuation, gRPC compatibility surface, or
task-store authority model.

## Strong boundaries

- `TaskId` cannot be substituted for `CommandId`, `EventSequence`, an SIR proposal identity, or an
  Oracle outcome identity.
- task source paths reuse validated `SirTaskArtifactPath`; source bytes remain a distinct bounded
  submission field and become typed content immediately at intake.
- mutation acknowledgements cite the exact `CommandId`; status/progress are read models and cannot
  authorize a workflow transition.
- an intent-review response binds the exact task, proposal, batch and individual request identities;
  the later decision binds the server-authenticated subject, exact caller scope and response.
- SDK wire deserialization reruns constructors; no derived decode bypasses invariants.

## Tests

- SDK strict V1/unknown-field/path/order/size/current-schema controls and compile-fail identity
  substitution tests;
- App API frame bounds, malformed canonical input, idempotent submit, changed retry input,
  status/not-found, cancel/replay, and reconnect cursor tests;
- server restart reconstructs submitted tasks and progress without an in-memory-only task list;
- CLI process tests exercise the same socket and server handlers as normal execution;
- SIR review never auto-selects an option and cannot cite a different task/proposal/request;
- log isolation and secret/source/prompt sentinel absence;
- live `cuda-samples` vectorAdd followed by reduction, both through `cairn-cli`, with exact SIR,
  Intent Admission, Oracle Exploration, qualified-control and Oracle outcome identities recorded.

## Explicit non-goals

- Candidate generation, build, Candidate Admission, NPU execution, or final migration verdict;
- knowledge/skill registry or retrieval;
- full-screen terminal rendering in the first slice (the reconnectable progress stream is its
  stable data seam);
- browser UI, public internet exposure, remote client enrollment, multi-tenant ownership, gRPC,
  or protocol-version negotiation;
- raw service-log export as authority or unrestricted artifact download;
- fixture answers, CUDA-sample-specific prompts, generic IDs, aliases, converters, dual readers,
  or compatibility layers.
