# Durable record, reconstruction, replay, and counterfactual design

- Status: normative focused design
- Date: 2026-08-24
- Parent design: [`SYSTEM_DESIGN.md`](SYSTEM_DESIGN.md)
- Requirements: `FR-REC-*`, `FR-AGENT-003..006`, `QR-AUD-*`, `QR-REL-*`

## 1. Purpose

Cairn makes expensive, nondeterministic decisions across model providers, local processes, remote
workers, containers, and accelerators. A useful record must do more than preserve chat text or a
final candidate. It must answer:

- What immutable task was being attempted?
- What did every model request contain, byte for byte?
- Why were those instructions, tools, history, and results selected?
- Which external effects were authorized, attempted, completed, or left ambiguous?
- Which artifacts and observations supported each admission and verdict?
- Can recorded external answers drive the workflow again?
- Can a historical boundary seed a new controlled branch, and where does it diverge?

The defining guarantee is:

> Every model-visible and verdict-relevant byte is either reconstructable from durable facts and
> immutable content or reported as a typed gap. Cairn never fills a gap by assumption.

This is a stronger and more precise statement than “logs exist.” It is deliberately weaker than
“live providers are deterministic.”

## 2. Sources of truth

Cairn has two durable truth stores:

1. **Event store** — ordered facts about decisions, lifecycle, authority, and relationships;
2. **Content-addressed store (CAS)** — immutable bytes cited by those facts.

Everything else is derived:

- relational/read-model tables;
- task and episode status;
- UI timelines;
- metrics;
- search indexes;
- snapshots;
- exported summaries;
- stored convenience fields such as `passed`.

A derived view may be rebuilt or repaired. It cannot override its source facts.

## 3. Identity types

Identity categories are distinct Rust types and wire schemas.

### 3.1 Content identity

`ContentId<T>` names exact bytes under an algorithm, semantic type domain, and canonicalization
contract. Reading the object recomputes and verifies the identity. A typed ID cannot deserialize a
wire identity carrying a different domain.

Examples: prompt block, tool catalog, task spec, source file, manifest, model response bytes,
execution receipt, oracle proposal.

### 3.2 Aggregate identity

`TaskId`, `EpisodeId`, `OperationId`, `JobId`, and similar identifiers name durable lifecycles. They
do not imply content bytes. Creation events explain their provenance.

### 3.3 Event identity

`EventId` names one canonical event envelope. Aggregate sequence establishes local order. Event IDs
and causal command IDs make duplicate delivery safe.

Trusted record code derives `EventId` after assigning aggregate sequence inside the append
transaction. The canonical identity material excludes `event_id` itself and includes stream,
sequence, schema, command identity, parent event, observation timestamp, and payload. The append
caller never supplies the resulting identity.

### 3.4 Derived identity

`DerivedId<T>` identifies a deterministic relationship or projection, such as an input identity
derived from continuation plus tool results. It records its domain separator and components. It is
not queried from CAS unless a separate content artifact was actually materialized.

### 3.5 Attempt identity

Logical work and concrete attempts differ. `JobId`/`OperationId` remain stable across authorized
retry; `AttemptId` changes. Evidence from failed or ambiguous attempts remains linked.

### 3.6 Physical blob identity

`BlobDigest` is an internal exact-byte digest used for storage integrity and physical deduplication.
It is not a semantic artifact identity and does not enter product APIs in place of `ContentId<T>`.
Two semantic types may have different content identities while sharing one physical blob.

### 3.7 Algorithm migration

Identity algorithms are tagged but V1 implements only SHA-256. A future upgrade is a controlled,
checkpointed migration that verifies old objects, writes new identities plus an immutable mapping
manifest, rebuilds mutable projections/indexes, and atomically cuts over the writer. Historical
events remain byte-identical. Legacy resolution uses the manifest at import/read boundaries rather
than adding general alias handling to every product path.

## 4. Event envelope and append contract

Every durable event includes:

```text
event_id
aggregate_kind
aggregate_id
aggregate_sequence
schema_name
schema_version
causal_command_id
caused_by_event_id? / parent_event_id?
observed_at
actor/session provenance?
payload bytes
```

The event ID covers canonical envelope fields and payload. `observed_at` is evidence and UI data,
not the authority for aggregate order.

Append requires an expected aggregate revision. Either the whole command's event batch commits or
none does. A command ID can be retried and returns its existing result without duplicating authority.

Cross-aggregate processes use explicit causality and idempotency; Cairn does not depend on a fragile
global timestamp order.

## 5. Command, event, and external-effect discipline

External effects are never hidden inside a reducer transaction.

The pattern is:

```text
command validated
  → intent/authority event committed
  → external effect attempted
  → result bytes captured
  → outcome event committed
```

A crash between effect and outcome leaves an attempt in an explicit state. Recovery behavior follows
the operation's effect class:

| Effect class | Example | Recovery |
|---|---|---|
| `Pure` | canonical projection | recompute |
| `ReadOnly` | CAS read, provider metadata lookup | retry with audit |
| `Idempotent` | content write by digest | retry with same key |
| `AtMostOnce` | metered provider request without idempotency | reconcile or request authority |
| `AmbiguousExternal` | remote command may have run before disconnect | inspect attempt evidence; never blind retry |

Human-readable error messages do not determine the effect class.

## 6. Content-addressed store

### 6.1 Canonical bytes

Structured artifacts define:

- schema name/version;
- canonical encoding;
- field ordering and unknown-field policy;
- identity algorithm/domain;
- maximum size and streaming behavior.

Text artifacts retain exact bytes; normalization is a separate derived artifact if needed. Directory
trees are canonical manifests with sorted paths, entry kinds, modes where meaningful, and content
identities.

V1 uses canonical UTF-8 JSON for structured bytes. The JSON codec is isolated from record semantics
and storage adapters. It rejects duplicate keys and ambiguous/non-canonical forms, defines number and
escaping behavior through fixtures, and represents verdict-critical floating-point observations with
exact bits, scaled integers, or specified strings. If a later codec is introduced, old objects remain
JSON objects under their original encoding identifier.

### 6.2 Writes and reads

CAS write:

1. stream to a temporary object;
2. compute identity;
3. verify caller expectation if supplied;
4. atomically publish or reuse an existing identical object;
5. return descriptor and observed size.

CAS read recomputes identity while streaming or through a verified cache. Missing, truncated, or
mismatched bytes are typed integrity failures.

### 6.3 Secrets

Credentials, API keys, private tokens, and secret environment values never become CAS objects or
event payload bytes. The record stores a secret-reference identity, provider/account label where
allowed, policy identity, and the fact that resolution occurred. Export rechecks redaction policy.

## 7. Durable event domains

### 7.1 Task and product facts

Examples:

- `TaskCreated`
- `TaskInputsResolved`
- `OracleSearchStarted`
- `OracleAdmitted`
- `CandidateSearchStarted`
- `VerdictAttached`
- `TaskCompleted`
- `TaskSuspended` / `TaskCancelled` / `TaskBudgetExhausted`

### 7.2 Agent facts

- `EpisodeOpened`
- `RoleScopeResolved`
- `TurnStarted` / `TurnEnded`
- `StepStarted` / `StepEnded`
- `TurnInputSelected`
- `ModelRequestPrepared`
- `ModelAttemptStarted`
- `ModelResponseReceived`
- `ModelResponseDecoded`
- `ToolCallProposed`
- operation lifecycle events
- `EpisodeCompleted`

### 7.3 Execution facts

- `JobDeclared`
- `AssignmentLeased`
- `AttemptStarted`
- `AttemptRunning`
- `EvidenceChunkCaptured` where durable streaming is required
- `AttemptOutputPublished`
- `AttemptCompleted` / `AttemptFailed` / `AttemptAmbiguous`
- `LeaseExpired`
- `AssignmentReconciled`

### 7.4 Verification facts

- `OracleProposalSubmitted`
- `AdmissionAttemptStarted`
- `VariantTrialRecorded`
- `MutationTrialRecorded`
- `CoverageEvaluated`
- `OracleRejected` / `OracleUnverifiable` / `OracleAdmitted`
- source/build/correctness/performance receipt attachments
- `CandidateVerdictProduced`
- `VerdictImpactAssessed` / `VerdictRetracted`

Schema names are illustrative until protocol types are implemented. The separation of durable fact
domains is normative.

## 8. Model input projection

### 8.1 Turn input decision

Before each model request, a `TurnInputPolicy` produces a canonical decision citing:

- ordered instruction block identities;
- selected model/deployment identity;
- selected tool catalog and schema identities;
- history policy and included event/item boundary;
- compacted summary identity and replaced range, if any;
- injected context identities and provenance;
- pending operation result identities and ordering;
- role/data/approval policy identities.

The decision is appended before request assembly. The request assembler reads only the committed
decision, durable history facts, and cited content.

### 8.2 Request materialization

The materialized request is stored as exact provider-facing bytes where the provider protocol has a
stable request body. When a provider SDK adds transport-only or secret fields, Cairn stores a
canonical secret-free request artifact plus enough adapter-version data to reconstruct the model-
visible request. The audit states which representation was used.

### 8.3 Provider continuation state

Provider-native continuation identifiers or opaque state are external facts. Cairn records:

- the continuation identity returned;
- provider/model/deployment;
- semantic history represented;
- request/response relationship;
- whether the state can be resumed, replayed only through recorded bytes, or not recovered.

Live continuation from a historical prefix must seed provider state from an authorized archived
continuation or rebuild a full request. Cairn verifies that the resulting request represents the
same recorded boundary before calling it a control.

## 9. Completeness audit

### 9.1 Backwards walk

For every model attempt, the auditor starts from `ModelRequestPrepared` and walks all referenced:

- input decision;
- instruction bytes;
- user/context bytes;
- tool schemas;
- conversation/history items;
- operation results;
- model configuration and adapter version;
- compacted summaries and their provenance;
- data-boundary policy.

For every verdict, it walks the evidence graph defined in `SYSTEM_DESIGN.md` and
`ORACLE_ADMISSION.md`.

### 9.2 Gap taxonomy

A gap is typed, for example:

- `MissingContent` — content identity has no bytes;
- `IntegrityMismatch` — bytes do not match identity;
- `UnarchivedModelInput` — model-visible input has no content identity;
- `UnknownSchemaVersion` — bytes exist but cannot be interpreted safely;
- `MissingDecision` — observed request cannot be tied to a recorded selection decision;
- `MissingOperationOutcome` — a referenced tool result is absent;
- `ExternalContinuationUnavailable` — provider-native state cannot be resumed;
- `SecretIntentionallyOmitted` — reconstruction boundary is known and policy-driven;
- `DerivedIdentityMisused` — code attempted to dereference a non-content identity;
- `EvidenceEdgeMissing` — verdict/admission manifest lacks a required citation.

The audit does not synthesize replacement bytes. A secret omission may be expected but still means
exact transport replay has limits; the report says so.

### 9.3 Runtime invariant

A model request cannot acquire dispatch authority until its audit projection is complete under the
task's declared reconstruction policy. This catches missing prompt/schema/context archiving before a
paid call rather than during later replay.

## 10. Replay taxonomy

The word replay is overloaded. Cairn exposes four distinct operations.

### 10.1 Byte replay

Reads archived raw provider response bytes and decodes them again through a selected adapter version.
It answers whether decoding behavior still produces the same semantic turn.

Expected nondeterministic sources replaced: provider transport only.

### 10.2 Semantic replay

Supplies archived semantic model turns and tool outcomes through recorded providers. It answers
whether the current agent loop reaches the same sequence of requests, tool calls, and state
transitions.

Expected nondeterministic sources replaced: provider semantics and tool effects.

### 10.3 Workflow replay

Reprojects a complete task or selected aggregates using recorded decisions and outcomes. It checks
process managers, job assembly, verification, and verdict graph behavior without re-executing
external effects unless explicitly selected.

### 10.4 Counterfactual continuation

Replays to a declared boundary, changes one named variable, and continues with live or alternative
providers/executors. This creates a new branch. It is an experiment, not a deterministic replay.

Examples:

- same request, live provider again — measures provider/control noise;
- model A replaced by model B;
- skill or instruction block added;
- tool catalog narrowed;
- context compacted;
- tool implementation replaced;
- oracle or admission policy changed for impact analysis;
- failure/recovery policy changed.

## 11. Recorded providers

Recorded provider implementations use ordinary runtime seams:

- `RecordedModelTransport` returns archived response bytes after verifying expected request identity;
- `RecordedModelAdapter` returns archived semantic turns after verifying semantic request identity;
- `RecordedToolGateway` verifies tool name/arguments/authority and returns archived outcomes;
- `RecordedTurnInputPolicy` returns archived decisions;
- `RecordedApprovalGateway` returns archived approval decisions;
- `RecordedExecutionBackend` returns archived attempt receipts where workflow replay allows it.

A mismatch is a typed divergence, not a reason to fall through to a live provider.

## 12. Branching and cut boundaries

### 12.1 Valid cut

A counterfactual cut occurs after a complete durable boundary, such as:

- an episode step with all operation outcomes committed;
- a candidate gate receipt;
- an oracle proposal/admission attempt;
- a task process-manager checkpoint.

Cuts do not occur inside an uncommitted external effect unless the experiment explicitly studies
ambiguous recovery.

### 12.2 Branch manifest

A branch cites:

- source task/episode/event boundary;
- replay modes used before the cut;
- control branch identity;
- exact perturbation descriptor;
- new provider/policy/artifact identities;
- authorization and cost budget;
- expected comparison metrics.

Historical aggregates remain terminal/immutable. The branch creates new aggregate identities with
lineage back to the source.

### 12.3 One-variable discipline

The framework records every identity changed between source/control/experiment. If more than the
declared variable changed, the experiment is marked confounded rather than silently attributed.

## 13. Trajectory comparison

Trajectory diff compares canonical semantic units, not timestamps or UI text alone.

It reports:

- longest shared prefix;
- first divergent request decision;
- first divergent model response/tool call/operation result;
- changed model-visible inputs before divergence;
- changed provider/environment/policy identities;
- each branch's terminal status/verdict;
- token, tool, time, device, and cost differences;
- branches that cannot be compared because of recording gaps.

Comparison identity includes the normalization/version rules used. A changed normalizer cannot
silently make historical trajectories appear equal.

## 14. Determinism claims

Cairn uses precise language:

- reducers and projections are deterministic functions of event/content bytes;
- content verification is deterministic;
- recorded external outcomes can drive deterministic workflow replay;
- local sandbox execution may still contain concurrency/hardware nondeterminism and must be measured;
- a live model continuation is nondeterministic even with byte-identical input;
- identical token count or provider cache reuse is evidence of request similarity, not identical
  response behavior.

The old observation that a reconstructed request produced the same input token count and a different
tool argument is the required control for this vocabulary.

## 15. Context compaction, skills, and model-visible transformations

Skills, compaction, dynamic tool selection, retrieved knowledge, and injected diagnostics are all
`TurnInputPolicy` decisions.

### 15.1 Compaction

A compaction event cites:

- source history boundary;
- compaction model/algorithm and exact input;
- summary bytes;
- replaced range;
- validation/approval if required.

Future model requests cite the summary and retained suffix. Replay can choose the recorded summary or
run a counterfactual compactor as a new branch. Compaction is not implemented until this record shape
exists.

### 15.2 Skills and knowledge

A skill/knowledge item is content-addressed. Selection is a durable input decision. Claims about its
utility are trajectory comparisons over declared controls, not author opinion or installation count.

## 16. Snapshots, indexing, and projections

Snapshots include aggregate identity, revision, last event ID, projection schema version, and state
bytes. Loading verifies the boundary and replays later events. An invalid snapshot is discarded.

Indexes support:

- identity and provenance lookup;
- task/episode timelines;
- artifact reverse references;
- verdict impact analysis;
- model-input audit;
- branch and divergence queries;
- lease/assignment operational views.

Indexes may be eventually consistent for queries but not for acquiring effect authority or producing
a verdict.

The V1 reference store is SQLite behind storage ports. SQLite owns no domain meaning: event append,
projection, coordination, and content contracts are tested independently of its tables. Replacing it
requires another adapter to pass the same crash, ordering, idempotency, lease, corruption, and rebuild
contract suites.

## 17. Export, retention, and garbage collection

### 17.1 Evidence export

An export manifest identifies:

- included aggregate event ranges;
- included content objects;
- omitted secret/external dependencies;
- schema and software versions;
- root task/verdict/oracle identities;
- integrity summary.

An offline verifier can check identities and rebuild supported projections without trusting the
exporting server's summary.

### 17.2 Retention

Retention policies may differ for raw model bytes, source, diagnostics, execution logs, and derived
views. Policy is known at task creation and changes are durable events.

### 17.3 Garbage collection

GC operates on an explicit root/reference graph. It never deletes by age alone while a retained
verdict, admission, branch, or required audit record references an object. Material deletion produces
an audit receipt identifying policy, roots considered, and removed identities.

## 18. Security and privacy

- CAS objects are untrusted input when read; identities and schemas are revalidated.
- Imported event logs do not grant execution authority.
- Replay defaults to no live external effects.
- Counterfactual live execution requires new authorization and budget.
- Tool/model content may contain prompt injection; authorization remains server-side policy.
- Export applies data policy and reports omitted material as reconstruction limits.
- Reasoning/private provider fields are stored only when provider terms and policy allow; their
  absence does not invalidate reconstruction of model-visible input, but may limit UI replay.

## 19. Compatibility

Persisted schemas are append-only by version. Readers:

- preserve unknown event bytes for export;
- reject unknown versions when required for a verdict or model-input projection;
- may skip explicitly non-authoritative unknown telemetry events;
- never assign a new meaning to an old field in place.

Migration creates new projection/snapshot schemas, not rewritten historical events. If canonical
bytes must change, a new artifact cites the old one and the transformation.

## 20. Initial acceptance controls

The record subsystem is not complete until all of these pass:

1. a task/episode can be rebuilt from events with projections deleted;
2. a complete historical model request is reconstructed byte-for-byte or with a verified canonical
   representation;
3. every model-visible input category is removed in turn and produces the correct typed gap;
4. derived identities are never queried as CAS content;
5. raw response bytes decode to the archived semantic turn;
6. recorded model and tool providers replay a full episode with zero unexplained divergence;
7. a wrong tool argument produces divergence rather than a live fallthrough;
8. a historical prefix creates an immutable branch and hands the live side the verified next input;
9. repeating the identical live request is recorded as a noise-floor control and may diverge;
10. a deliberate context/tool/model perturbation reports the first divergence and all changed
    identities;
11. a crash at every intent/effect/outcome boundary recovers to the correct completed or ambiguous
    state;
12. a tampered summary, content object, event payload, or snapshot is rejected;
13. secret references remain usable without secret bytes entering export/CAS;
14. a verdict export can be verified offline from its manifest and included objects.
