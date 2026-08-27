# Resolved design decisions

- Status: normative decision register
- Date: 2026-08-27

This register records decisions that close entries in [`OPEN_QUESTIONS.md`](OPEN_QUESTIONS.md).
Detailed requirements remain in `SYSTEM_REQUIREMENTS.md`; detailed implementation boundaries remain
in `SYSTEM_DESIGN.md`.

## D-001 — Canonical JSON first, behind a codec boundary

- Resolves: OQ-001
- Decision: accepted

Cairn V1 will use canonical UTF-8 JSON for persisted structured artifacts, event payloads, fixtures,
and exported manifests.

The choice of JSON is an adapter decision, not a license for serialization details to spread through
domain and workflow code:

- domain types and state machines live in format-neutral crates;
- `cairn-codec` owns canonical JSON encoding/decoding, strict V1 dispatch, and conformance fixtures;
- persisted envelopes carry schema and encoding identifiers;
- content/event identity is computed from canonical encoded bytes under an explicit domain;
- a future encoding is introduced as a new codec/version and explicit transformation, never by
  silently changing the bytes an old identity means.

Canonical JSON V1 will define object-key ordering, UTF-8 handling, escaping, duplicate-key rejection,
number representation, and unknown-field behavior. Verdict-critical floating-point observations
will use exact integer bits, scaled integers, or specified strings instead of depending on a JSON
parser's floating-point round trip.

If “JSONP” means the browser callback format, it is not a persistence format: it may be a client-side
presentation wrapper someday, but it will not participate in canonical identity or durable storage.

## D-002 — SQLite first, behind storage ports

- Resolves: OQ-002
- Decision: accepted

Cairn V1 will use SQLite as the reference and production-first implementation for:

- append-only event streams and expected-revision checks;
- command idempotency and transactional outbox state;
- task/episode/operation projections;
- jobs, leases, attempts, and reconciliation state;
- artifact metadata and reference graph;
- subscription cursors and operational indexes.

Persistence is isolated behind narrow contracts:

- `EventStore` — append/read streams with expected revision and command idempotency;
- `ProjectionStore` — rebuildable views and checkpoints;
- `CoordinationStore` — leases, outbox, and process-manager checkpoints;
- `ContentStore` — immutable byte streaming and identity verification.

`cairn-store-sqlite` implements the first three. The initial `ContentStore` may use filesystem blobs
with SQLite metadata; callers do not depend on that layout. Product, agent, execution, verification,
and API crates depend on ports rather than `rusqlite` or SQL schemas.

SQLite-specific features such as WAL, busy timeout, transaction shape, and backup remain adapter
details. A future PostgreSQL or remote content-store adapter must pass the same contract and fault-
injection suites before it can replace SQLite.

## D-003 — Hybrid evidence for the structured domain

- Resolves: OQ-003
- Decision: accepted

The supported domain is resolved from several separately recorded authorities rather than authored
entirely by one agent or required in full from one caller.

### Caller responsibility

The caller supplies a minimum machine-readable contract covering, for the requested claim:

- source entry point and parameter/buffer roles;
- dtypes and known shape/rank variables;
- declared valid ranges and required invalid/error behavior;
- requested output semantics or an independent semantic reference;
- explicit exclusions and unknowns.

The contract may be incomplete, but it may not disguise an unknown as an unrestricted domain.

### Cairn responsibility

The isolated Semantic Intent Recovery subsystem may propose refinements from source, documentation,
framework definitions, caller/model context, prior feedback, and upstream tests. Each refinement
cites its evidence and remains distinct from the caller declaration. Under D-025, this subsystem is
proposal-only; Blue receives the subsequently admitted intent and cannot perform this promotion.

Cairn independently challenges both through:

- source implementation interrogation and boundary probing;
- mandatory cases derived from the caller's minimum contract;
- upstream and external test proposals with provenance;
- historical target-failure coverage obligations.

### Conflict rule

Caller declaration, SIR hypotheses, source observations, external expectations, and production
feedback are never overwritten into one unattributed value. Intent Admission records agreements and
disagreements. An unresolved disagreement affecting the requested intent produces `Conflict`,
`Unknown`, or `NeedsUserDecision`; it cannot be repaired by Oracle Blue.

The admitted structured domain is part of the immutable `MigrationIntentContract`. Oracle Admission
may admit only claim domains consistent with it. Changing the contract creates new intent, Oracle,
and experiment identities.

## D-004 — Policy-configured variant sufficiency

- Resolves: OQ-005
- Decision: accepted

There is no universal correct count of correct-by-construction and deliberately incorrect variants.
Variant sufficiency is defined by the immutable, versioned `AdmissionPolicy` selected for an
admission attempt.

The policy configures at least:

- minimum accepted correct-by-construction variants;
- minimum rejected implementation-level incorrect variants;
- required applicable fault/construction classes;
- structural-independence requirements;
- saturation rounds and what resets saturation;
- provider/execution budgets and the outcome when they are exhausted.

The V1 reference profile starts with two structurally distinct correct variants, three deliberately
incorrect variants, applicable fault-class coverage, and two consecutive saturation rounds. These
are profile defaults, not hard-coded verifier constants.

Counts are necessary controls, not sufficient evidence. Every accepted correct variant still needs
an independent `ConstructionClaim`; every mandatory incorrect variant must be rejected through the
required execution scope; and the generic mutant/case grid remains a separate obligation. A policy
that requests a stronger claim may increase these requirements. A profile may configure a smaller
family only if its accepted strength and limitations explicitly permit that reduction; it cannot
label missing evidence as having passed.

Budget exhaustion before the selected policy is satisfied yields rejection, a permitted weaker
strength, or `Unverifiable`. Cost limits never turn incomplete admission into `Admitted`. The exact
policy identity and observed stopping reason are retained in the admission receipt.

## D-005 — Separate numerical provenance from assurance

- Resolves: OQ-008
- Decision: accepted

A numerical allowance records two orthogonal classifications:

1. **provenance** records where the allowance came from, such as `MeasuredFamily`,
   `MeasuredAdversarial`, `ExternalPrior`, `Asserted`, or `ExactOrSet`;
2. **assurance** records what the evidence justifies: `ProvenBound`, `ExhaustiveFinite`,
   `HeldOutValidated`, `ExploratoryMeasured`, `PriorOnly`, or `Unsupported`.

`HeldOutValidated` requires identity-disjoint derivation and validation corpora. The receipt records
variant count and independence, input-generation/search strategy, observed spans, seeds, domain
regions, and known unexplored regions. A safety factor is policy, not proof, and does not promote an
empirical maximum to `ProvenBound`.

`HeldOutValidated` may support an empirical `Satisfied` claim outcome when allowed by the selected
admission policy.
The verdict must label it as empirical and state its admitted domain, corpus relationship,
allowance, and limitations. Only a justified `ProvenBound` or `ExhaustiveFinite` assurance may use
an unqualified domain-wide numerical claim. `ExploratoryMeasured`, `PriorOnly`, and `Unsupported`
cannot be presented as such a claim.

Cairn does not report probabilistic confidence unless the input distribution and sampling procedure
are themselves declared and justified. A measured maximum alone is an observation, not a
statistical or mathematical bound.

## D-006 — MIT license for the public project

- Partially resolves: OQ-017 (outbound project license)
- Decision: accepted

Cairn source code and project-authored documentation will be released under the MIT License. Root
license text, Rust package metadata, generated release metadata, and source/fixture provenance must
agree with that choice.

This decision does not silently relicense imported code, corpora, model outputs, vendor SDKs, or
historical Cairn/Alloyport material. Such material requires compatible provenance or must remain an
external/private input. Contribution certification, governance, trademark policy, and the detailed
dependency-license gate remain to be decided before the first public release.

## D-007 — Typed SHA-256 identities with a pre-release V1 reset policy

- Resolves: OQ-013
- Decision: accepted

Cairn V1 uses SHA-256 for content, event, and deterministic derived identities. Every public
identity carries an explicit frame version, algorithm tag, and semantic domain; Rust APIs retain the
domain as a concrete type such as `ContentId<SourceFile>` rather than an untyped digest.

The hash preimage is a versioned, length-delimited binary frame containing a fixed Cairn magic,
frame version, algorithm identifier, semantic domain, and exact canonical/payload bytes. Domains
are stable registered constants. Hashing uses a maintained cryptographic library and published test
vectors; Cairn does not implement SHA-256 itself.

Lifecycle identities such as task, episode, operation, job, attempt, command, and branch are
distinct UUIDv7-backed Rust types. UUID timestamp ordering is an operational convenience only and
never establishes event order, causality, or authority.

### Semantic and physical identities

- `ContentId<T>` is the public, domain-separated semantic identity for exact bytes under type `T`;
- `EventId` and `DerivedId<T>` are domain-separated identities for canonical relationship bytes;
- `BlobDigest` is an internal exact-byte storage/integrity digest used for physical lookup and
  deduplication.

The same bytes used under two semantic types therefore have different `ContentId<T>` values but may
share one `BlobDigest`. Product APIs never substitute `BlobDigest` for a semantic identity.

An `EventId` covers the canonical envelope without its own identity field, including stream,
sequence, schema, command causality, parent, observation time, and payload. Because sequence is
assigned under optimistic concurrency, trusted record code derives the event identity after
sequence allocation inside the append transaction; an untrusted caller does not supply it.

### Pre-release algorithm changes

V1 implements a closed SHA-256 algorithm enum rather than a speculative pluggable hashing
framework. Until Cairn publishes its first compatibility baseline, an algorithm or frame change
replaces the current V1 definition. Development databases and artifacts are explicitly rebuilt;
runtime readers do not translate, alias, or auto-upgrade an earlier development format. The
post-release algorithm-upgrade policy is deliberately deferred to a new decision made with real
retention and deployment constraints.

## D-008 — Semantic turns plus protocol-native continuation

- Resolves: OQ-012
- Decision: accepted

Cairn uses a hybrid record for model interaction. A provider-neutral semantic turn is the derived
contract consumed by the agent loop, tool validation, inspection, and semantic replay. Alongside it,
the selected protocol codec archives a lossless, versioned native continuation containing the
ordered protocol items and correlation identities required to materialize the next request.

There is deliberately no universal lossy `Message` type standing in for all protocols. OpenAI
Responses retains typed response items and function `call_id` relationships; OpenAI Chat
Completions retains the exact assistant message and `tool_call_id` relationships; Anthropic
Messages retains ordered content blocks and `tool_use_id` relationships. Reasoning, redacted, and
unknown policy-allowed native items are preserved without granting them semantic authority.

Reasoning replay behavior is a model-template characteristic. Responses profiles either preserve
all returned output items or additionally request OpenAI encrypted reasoning for stateless replay;
DeepSeek Chat profiles require `reasoning_content` whenever an assistant message contains tool
calls; Anthropic Messages preserves signed/redacted thinking blocks in their original order. A
missing required field is a completeness error before dispatch, never a reason to synthesize state.

V1 reconstructs continuation locally from archived material. Responses uses full input with hosted
storage disabled; Chat Completions and Anthropic Messages rebuild their full native message/block
history. Provider response IDs and hosted continuation state may be recorded as external facts, but
are neither required for reconstruction nor treated as durable authority. A later hosted-state
optimization must retain a local fallback and prove the represented history boundary.

A catalog alias resolves a versioned model template, deployment, protocol, settings, transport
bounds, and a credential reference into a frozen secret-free snapshot. Protocol—not provider or
model name—selects the codec. Switching model, deployment, protocol, template revision, or codec
version creates a new episode or an explicit counterfactual branch.

## D-009 — Repository model templates and user deployment configuration

- Decision: accepted

Model characteristics are project-maintained versioned data under `model-templates/`, not fields an
operator must copy into runtime configuration. A template owns the provider-visible model name and a
separate section for every supported protocol. Each section declares context/output limits, tool,
parallel-call and reasoning capabilities, accepted reasoning efforts, schema dialect, safe defaults,
and protocol-specific request settings.

User runtime configuration owns only operational choices: enabled template alias, selected protocol,
endpoint, provider/account label, credential reference, data boundary, transport bounds, and optional
generation overrides. This permits a private deployment to reuse the same model template without
claiming that its endpoint defines the model's intrinsic capabilities. Overrides are validated
against the selected protocol section.

Resolution includes the exact typed template content identity and materialized characteristics in a
secret-free episode snapshot. A template update changes new episodes only. Endpoint conformance is a
separate measured fact and may later narrow or reject a configured deployment; it does not require
operators to maintain a duplicate capability profile.

## D-010 — Resource-driven placement, not worker business roles

- Decision: accepted

Workers have stable identity, controller-authorized pool membership, observed resources, execution
backends, dynamic capacity, and evidence provenance. They do not have oracle, candidate, source,
target, or migration-stage roles. Agent roles remain episode capabilities; migration stages remain
product orchestration metadata.

`cairn-migration` decides which evidence and opaque execution are required and emits a generic
placement request. The execution scheduler selects a concrete worker by platform, allowed pool,
backend, capabilities, availability, trust, and later cost policy. Native platform facts come from
runtime/binary probes; operator configuration may constrain expected values but cannot replace the
observation. Pool membership comes from controller enrollment authority rather than worker report.

## D-011 — Local worker keys and enrollment bootstrap

- Decision: accepted; online reference path implemented

A worker private key is generated and retained on the worker. The normal online path accepts one
short-lived enrollment bundle, submits a CSR over a pinned/authenticated controller channel, and
persists the issued credential, trust anchor, stable `WorkerId`, and configuration under one state
directory. Operators do not copy private keys or hand-author lifecycle identities.

The identity chain remains explicit: one-shot `EnrollmentId`, stable `WorkerId`, rotatable
credential identity/serial, per-process `WorkerIncarnationId`, and per-connection
`ControlConnectionId`. Offline CSR exchange and external issuers such as enterprise CA or SPIFFE
must fit behind the same issuer port. A built-in CA may be the open-source default, but it does not
grant permission to conflate a certificate fingerprint with permanent worker identity.

The implemented reference path uses a separate server-authenticated TLS listener, a controller
file-backed issuer adapter, and one append-only registry stream. Only a token digest is durable. A
committed issuance retains the exact CSR digest and public credential result, so a lost response is
recoverable only with the original staged worker key/CSR. Offline exchange and non-file-backed
issuers remain later slices.

## D-012 — Application credential authority precedes scheduler authority

- Decision: accepted; revocation foundation implemented

The certificate fingerprint is credential evidence, not the permanent principal. Authentication
maps an accepted managed certificate to a stable subject derived from the controller-owned
`WorkerId`, an exact rotatable `CredentialId`, and an authorized pool. Registration permanently
binds subject and pool but binds the credential only to the current incarnation.

Credential revocation, logical-worker disablement, and unused-enrollment revocation are separate
append-only facts. TLS chain acceptance does not override these application facts. The baseline
does not require CRL or OCSP; external issuer adapters may add them without changing Cairn's domain
lifecycle. Scheduler work follows this foundation so a placement snapshot can exclude inactive
authority without guessing from certificate expiry or connection state.

## D-013 — Rotation is successor issuance plus bounded predecessor authority

- Decision: accepted; online reference path implemented

A rotation authority names one exact active predecessor credential, not merely a worker. Issuance
creates a fresh local key and successor `CredentialId` while preserving controller-owned
`WorkerId`, stable principal, and pool. The controller freezes the configured optional overlap at
issuance time. With an overlap, predecessor authority ends at that exact instant; with `null`, it
remains active until explicit revocation.

Worker material is immutable under a per-rotation directory. Cutover is one atomic replacement of
`identity.json`; a running worker polls that manifest at a configured positive interval, closes the
old connection, reloads the successor, and creates a new incarnation. Revoking a bad successor
inside its overlap atomically cancels predecessor retirement in the registry, after which the local
manifest may be rolled back. Once the frozen deadline passes, rollback cannot resurrect authority.

## D-014 — Scheduling choice and capacity authority are separate durable identities

- Decision: accepted; C1 kernel implemented

One `PlacementId` identifies an immutable evaluation over a frozen, content-addressed candidate
snapshot. One `ReservationId` is the separate authority that consumes a worker slot before the
downstream `AssignmentId`/`LeaseId` can be created. A snapshot records the exact incarnation,
credential, profile, availability, last heartbeat, controller-authority revision when available,
configured policy, every rejection, and the deterministic stable-`WorkerId` choice.

The V1 scheduler uses one append-only global reservation ledger. This intentionally pays a
serialization cost to make concurrent capacity admission unambiguous with the current event-store
port; optimistic revision conflict cannot turn into double reservation. A reservation is not freed
merely because a clock elapsed: it remains authoritative for live or in-doubt execution. Release
requires durable terminal/pre-start-expiry evidence, or proof that no assignment claimed it before
the separately configured positive claim deadline. Heartbeats prevent double subtraction by naming
active attempts, while pending/unreflected reservations still reduce reported availability. One
attempt cannot hold parallel active reservations. Future sharding must preserve these identities and
proof obligations.

## D-015 — Worker pool is independently mutable registry authority

- Decision: accepted; lifecycle mutation path implemented

Pool is neither a worker self-asserted resource nor an immutable credential attribute. The
registry owns one current pool projection per stable `WorkerId`, with the event establishing that
revision retained independently from credential issuance. Reassignment requires explicit worker
disablement, an actual target change, and its own idempotent `CommandId`; re-enable is a separate
fact. This ordering gives operators a fail-closed maintenance window without deleting credential
or worker history.

Execution history does not infer this change from a reconnect. Before registration in a changed
pool, the controller appends a typed cross-link citing the exact registry assignment event. The
execution projector accepts it only for a disconnected or exactly expired predecessor session.
Consequently a stale startup cache, worker hello, certificate renewal, or ordinary process restart
cannot silently move scheduling authority between pools.

The operator read surface projects these same facts on demand rather than maintaining a mutable
administrative table. List/show reports retain strong identity and provenance links; audit succeeds
only after complete causal replay. This keeps operational visibility from becoming a second source
of worker or credential authority.

## D-016 — Join bundle owns bootstrap-to-control endpoint handoff

- Decision: accepted; F1 implemented

A one-command join cannot safely infer the normal controller address, TLS name, or trust anchor
from a bootstrap listener. The short-lived bundle therefore carries separate typed bootstrap and
ordinary-control endpoint descriptions. They may intentionally use different listeners, DNS names,
and server CAs. This endpoint material is public configuration; the bearer secret remains the only
bundle capability and only its digest is durable at the controller.

Join composes existing enrollment and probe ports into a fixed worker state tree. On first success
it writes an editable strict configuration; later runs validate and reuse that file rather than
regenerate it, so binary upgrades or operator tuning do not become destructive bootstrap actions.
Enrollment authority establishes identity and pool membership, not backend correctness or
readiness. The generated worker therefore fails closed as unavailable/draining until a separate
explicit activation path configures a real executor.

## D-017 — Worker-local typed material is a prerequisite for execution authority

- Decision: accepted; F2a implemented and preserved by F2b

An assignment identity alone does not prove that a worker can execute its frozen contract. The
controller therefore loads and verifies the exact typed input bundle and execution environment from
authoritative CAS and places their identities and lengths in the durable offer. The worker must
derive the expected type-tagged identities and commit both objects to its independent local CAS
before it may persist assignment
admission. It must read and verify them again from local CAS before it may persist execution start.
An in-memory acknowledgement or controller-side content binding cannot substitute for that local
proof.

## D-018 — Chunk transfer is resumable data movement, not execution history

- Decision: accepted; F2b implemented

The durable offer freezes a compact typed manifest, not artifact bytes or one event per chunk. An
authenticated worker may request sequential bounded ranges only while that exact offer remains in
the controller outbox. Controller delivery is one in-flight logical message per worker connection,
so another control message cannot be mistaken for a chunk response. Range traffic is ephemeral and
repeatable; acknowledging the offer is deliberately delayed until complete local-CAS verification
and assignment admission.

Each response is canonical unpadded-base64 JSON under control protocol V1. Worker staging is a
fixed private per-offer directory; every append is synced, its regular-file length is the restart
cursor, and invalid range metadata fails before append. The authoritative controller object is
fully verified once when building the manifest. A replaceable `ContentRangeStore` port then avoids
O(n²) rescans, while the worker's final full `ContentId<T>` derivation detects source/storage/wire
corruption. Aggregate controller/worker limits remain independently optional. Positive controller
and worker chunk sizes are explicit, and exact base64 envelope expansion must fit any enabled
transport bound before startup.

## D-019 — F2 uses one concrete Docker adapter

- Decision: accepted; F2 implemented and measured

A typed content ID prevents cross-domain substitution but does not define how an executor interprets
bytes. `InputBundleV1` therefore admits only canonical sorted explicit directories and regular files
under strong `SandboxPath` values. `DockerExecutionEnvironmentV1` pins a full immutable local image
ID and canonical environment variables.

Execution activation is one coherent configuration invariant: disabled mode advertises only
`transport-only` and remains unavailable/draining with zero slots; Docker mode advertises only
`docker-v1` and is ready, non-draining, with concurrency and availability both one. Optional
deployment resource limits are configuration, not fixed policy.

One deterministic container belongs to one `AttemptId`. Worker restart reconciles that container
instead of manufacturing a new execution authority. Terminal publication precedes cleanup. The
adapter assumes trusted private infrastructure and is deliberately not wrapped in a provider-neutral
OCI runtime framework or presented as malicious-code containment.

## D-020 — Oracle blue and red use separate cache-aware episodes

- Decision: accepted

Blue and Red name Cairn's current model-backed synthesis and adversarial strategy profiles; they are
not permanent required Agent types or deployment processes. A policy may substitute deterministic
mutation, property, counterexample, or other admitted exploration strategies. Whenever both
model-backed profiles are used, the following isolation remains mandatory.

When these profiles are selected, one oracle-search attempt opens distinct blue and red Cairn
episodes. They have separate
`EpisodeId` values, durable histories, model snapshots, budgets, capability sets, and visibility
policies. Blue and red exchange only submitted content-addressed artifacts and trusted diagnostic
bundles; a provider-native continuation or unsubmitted reasoning never crosses the role boundary.

This separation is logical and durable, not a requirement to depend on provider-hosted conversation
state. V1 continues to reconstruct stateless requests locally. Prompt-cache efficiency is preserved
by deterministic, append-only role projections with stable instructions, tool ordering, caller
contract, source snapshot, and policy material before changing evidence. Cache reuse across roles is
an optional provider optimization and never justifies exposing a capability or private history.

Provider-returned cache read/write/miss token counts are recorded when supplied. Missing detail is
unknown rather than zero, and cache reuse is not replay or correctness evidence. Cost evaluation
uses total input, uncached input, output, latency, and task quality rather than hit percentage alone.

## D-021 — External tests enter through a bounded blue research tool

- Decision: accepted

The current model-backed synthesis (Blue) profile receives a read-only
`oracle_search_external_tests` product tool. The model supplies a
bounded query and operator-approved repository scopes; trusted adapters own endpoints, credentials,
redirect policy, response bounds, and repository allowlists. Results retain exact query, source,
immutable upstream revision/blob identity where available, exact fetched bytes, retrieval
provenance, and truncation. Exact bytes are archived for reconstruction; the model receives a
deterministically selected, line-addressed excerpt capped per result and bound to the exact byte and
search-result identities. Full source files are never copied into a prompt merely because code
search matched them.

PyTorch and other upstream tests are research context, not truth or import candidates. Search
snippets and fetched bytes cannot become executable cases; Blue must independently author Cairn's
structured test proposal and may cite only the research-result identity that informed it. The tool
does not query repository licenses because this path does not vendor or distribute upstream bytes.
Any future importing workflow remains subject to the ordinary release controls in D-006. Recorded
providers drive offline CI and replay through the same generic tool-operation seam as live
providers.

## D-022 — Oracle disagreement uses bounded artifact-mediated debate

- Decision: accepted

When policy selects the current model-backed synthesis/adversarial revision strategy, Blue and Red
retain separate durable episodes across an OracleSearch attempt. Red reviews exactly
one frozen Blue revision. A Red `revise` with blocking findings is returned as trusted structured
feedback to Blue's existing episode; Blue must submit a complete changed replacement, which creates
a new immutable identity before Red can review it. Neither private continuation crosses the role
boundary, and malformed submissions are rejected atomically with exact diagnostics to the role
that produced them.

The loop has separately configured limits for Blue submission repair, Red submission repair,
Blue/Red adversarial rounds, and blocker-free stability rechecks. A blocker-free review status
requires an empty blocker set and the configured rechecks over the same frozen revision; it does not
constitute Admission. A later concrete blocker reopens revision; repeated votes never manufacture
admission. Limit exhaustion produces an explicit non-converged terminal result for trusted admission
or an operator to consume.

This is not an instruction to maximize conversation length. Additional turns are purchased only
to repair a rejected structure, resolve a concrete blocker, or test a named instability surface.
The stable common and role instructions remain content-addressed prefixes; frozen artifacts and
trusted diagnostics occupy the changing suffix so useful provider cache reuse does not require
merging the two roles.

## D-023 — Structured logs are an operational projection, not durable authority

- Decision: accepted

Cairn uses `tracing` events for live operator visibility and retains append-only domain events plus
typed content as the only reconstruction, retry, scheduling, and verdict authority. Logging may be
filtered, delayed, duplicated, collected, rotated, or lost without changing domain behavior. No
component reads logs back into a state machine.

Process binaries initialize one stderr subscriber. JSON is the default encoding; compact text is an
operator option. `CAIRN_LOG` selects target/level directives and `CAIRN_LOG_FORMAT` selects `json`
or `compact`. Machine-readable CLI results remain on stdout. Cairn does not implement its own log
file rotation in V1; the process supervisor or collector owns transport, retention, and rotation.

Operational events use stable names and typed identity values for correlation. INFO covers process
and work lifecycle transitions, WARN covers recoverable or terminal adverse outcomes, ERROR covers
process/subsystem termination, and DEBUG covers periodic heartbeat/resource or wire-adjacent
detail. Request/response bodies, prompts, reasoning, credentials, tool arguments/results, source
bytes, and workload output bytes are forbidden. Logs may state that a diagnostic was archived and
where its typed identity or attempt can be found, but generic boundaries do not print opaque
diagnostic text that might contain a secret.

Logging is also semantically subordinate in source layout. An event may borrow an immutable typed
identity or an already-computed outcome and may compute only an infallible bounded scalar
projection. It may not invoke an external capability, generate an identity, obtain an authoritative
clock value, classify a lifecycle outcome a second time, mutate state, await, or propagate an
error. Business operations commit outside logging constructs. Cairn V1 does not use tracing spans
or `instrument` in business crates, so deleting an observability scope cannot delete the work it
described. A future distributed-tracing decision must preserve this ownership rule explicitly.

## D-024 — Cairn product scope is CUDA → Ascend C

- Decision: accepted

Cairn is specifically a CUDA-to-Ascend-C operator migration system. Its product language,
requirements, evaluation corpus, domain adapters, Oracle risks, performance model, and public claims
remain inside that boundary. Domain-neutral agent, record, execution, and verification mechanics are
internal dependency properties; they are not evidence for a general heterogeneous-migration product.

A second materially different CUDA operator is required to validate the internal seams. Supporting
another source or target requires a future explicit product decision. Generic names or hypothetical
reuse do not authorize abstractions for that future.

## D-025 — Higher-order intent recovery is isolated and proposal-only

- Decision: accepted

Cairn's primary semantic objective is to recover the user's higher-order algorithm, numerical,
model/deployment, and observable-contract intent rather than mechanically preserve CUDA implementation
choices. `Semantic Intent Recovery` is an independently replaceable subsystem that may read a wider,
bounded context than the single-kernel execution unit and may emit competing evidence-backed
hypotheses, conflicts, unknowns, invariants, and optimization freedoms.

Its output has proposal types and cannot be passed where an admitted migration-intent contract is
required. Separate Intent Admission promotes exact claims. The extractor cannot read hidden
admission material, modify caller or admitted contracts, or decide an Oracle/candidate verdict.

## D-026 — Oracle and verdicts are multi-plane, claim-scoped portfolios

- Decision: accepted

Algorithmic correctness, numerical acceptance, execution/integration authenticity, memory and
concurrency safety, Oracle adequacy, and performance are distinct planes. User-facing summaries may
group execution and safety under correctness, but the underlying claims, evidence, outcomes, and
blind spots remain separate. The performance plane is always present in the portfolio. When the
caller supplies no business target it may remain informational, unknown, or not executed, but it is
not silently omitted. It can never compensate for a required correctness-plane failure.

Oracle and candidate outcomes are claim- and domain-scoped, with explicit satisfied, violated,
unknown, conflict, not-applicable, not-executed, and infrastructure-failure states plus a discrete
evidence strength. A policy may derive a release decision, but no global confidence scalar or stored
`passed` bit replaces those facts.

## D-027 — Performance uses admitted conditional ceilings

- Decision: accepted

Hardware performance is modeled as a family of conditionally applicable ceilings rather than a
single device roof. Cairn distinguishes official/theoretical facts, controlled microbench
measurements, algorithmic rooflines, implementation rooflines, candidate observations, and business
targets. Claims bind SoC, dtype, shape, engine, memory path, dataflow, concurrency, toolchain, device
state, workload, and measurement policy.

An optional `PerformanceExperimentPlannerProfile` may use an agent, but profiler interpretation,
unit conversion, measurement validity, comparison, and final performance outcome are recomputed
from authoritative receipts.

## D-028 — Previous-iteration and real-model feedback is typed evidence

- Decision: accepted

Candidate counterexamples, Oracle false accepts/rejects, profiling, integration results, production
observations, user decisions, and coverage gaps enter subsequent exploration as different typed
evidence. They are not an untyped reward and do not mutate an admitted intent, Oracle, threshold,
corpus weight, or historical verdict in place.

Positive model-level behavior is weak support for its exact deployment slice and does not prove local
kernel correctness. Negative behavior creates a valuable regression obligation but retains
attribution uncertainty until first-divergence or equivalent evidence resolves it.

## D-029 — Knowledge and skills have claim/content-scoped trust lifecycles

- Decision: accepted

Cairn adopts T0 specification/machine facts, T1 measured facts, T2 validated mechanisms/recipes, and
T3 task cases/feedback as distinct knowledge classes. Each exact knowledge claim records provenance,
scope, dependencies, evidence strength, lifecycle, conflicts, freshness, and allowed uses. Author,
vendor, official, built-in, and retrieval rank are never trust by themselves.

Skills have an independent unaudited/reviewed/validated/refuted lifecycle. Reviewed but unvalidated
skills may help inside bounded exploration to avoid a validation-use deadlock, but they cannot
support admission-critical claims, modify judge policy, or expand role capabilities. Content change
invalidates validation for new runs. Retraction remains auditable and propagates through reverse
references to affected intent, Oracle, performance, and verdict artifacts.

## D-030 — Authority and source-behavior disposition are claim-scoped

- Decision: accepted

Cairn has no global authority ranking across intent, mathematical truth, source behavior, device
observation, and model proposal. The user decides desired semantics and policy within an explicit
scope; execution receipts decide what ran and was observed; an admitted specification/reference
supports only its exact claim; models, knowledge, skills, and retrieval remain proposals.

Every anomalous, undefined, specialized, or intent-divergent CUDA region receives an admitted typed
disposition: preserve observed behavior, follow admitted semantic intent, exclude the undefined
region, split the domain, or block pending user decision. There is no product-wide preserve-or-fix
boolean.

## D-031 — Hidden evidence burns on disclosure and feedback cannot self-create held-out strength

- Decision: accepted

Hidden cases carry sealed, consumed-without-disclosure, burned-to-public-regression, or retired
state plus an exposure ledger. If a diagnostic reveals enough distinguishing information to the
applicant lineage, the case loses hidden strength, becomes a public regression, and is replenished
when its coverage partition remains required. Repeated pass/fail querying is treated as possible
disclosure rather than a harmless API.

Feedback has an explicit allowed-use disposition and contamination graph. Applicant-visible,
derivation-equivalent, or knowledge-injected feedback cannot be relabeled as held-out evidence for
the same claim under a new content identity.

## D-032 — Admission mechanisms and policies require qualification

- Decision: accepted

Repository ownership defines the trusted-computing boundary but does not prove a comparator, runner,
adapter, parser, sanitizer/profiler adapter, corpus builder, gate, diagnostic redactor, aggregation
rule, or admission policy correct. Each verdict-relevant mechanism has exact identity, scoped
qualification evidence, lifecycle, limitations, and requalification triggers.

The lowest trust root is kept small and supported by independent tests, review, mutation/fault
injection, real-tool calibration, and replayable receipts. A gate cannot certify itself and a second
agent's agreement is not mechanism qualification. Refutation triggers reverse impact analysis.

## D-033 — Normative document conflicts block implementation

- Decision: accepted

Requirements define observable obligations, decisions record accepted choices, the system design
defines overall authority, and focused designs add detail without weakening them. Implementation
plans, current code, historical fixtures, research reports, and open questions do not override this
normative set.

A real conflict among normative documents blocks the affected implementation slice until all
impacted documents are reconciled. Cairn does not add fallback behavior or dual interpretations to
paper over a design conflict during pre-release V1 development.

## D-034 — The control plane is modular, while proposal and admission authorities cross processes

- Decision: accepted

Cairn keeps workflow, public record/CAS, scheduling, API, feedback routing, knowledge/skill registry,
and the initial deterministic Hardware Performance Model in a modular Controller. It does not turn
every domain concept into a microservice.

Semantic Intent Recovery runs as a separate OS process from the first new-architecture V1 slice.
Oracle synthesis/adversarial strategies, typed Admission Planners, and Candidate Search run as
isolated durable episodes outside Admission authority. Capability-equivalent episodes may share a
Proposal/Planning Host, while a different data/tool/OS capability boundary requires a different
process instance. The mechanical Admission gate and restricted material run in a separate authority
process. A process boundary is required by replaceability, hidden-data visibility, execution risk,
or promotion authority—not by Agent count or module name.

## D-035 — Public, restricted-admission, and secret storage are separate capabilities

- Decision: accepted

Cairn exposes no universal content-store handle where possession of a content identity implies read
authority. Public evidence, restricted admission material, and secret references use distinct typed
ports, identities, process credentials, and lifecycle policies. A minimal deployment may use the
same storage technology and host, but public and restricted data use separate database files/CAS
roots and cannot be opened by the same ordinary application capability.

Hidden device jobs use Controller scheduling metadata plus a one-time, attempt-scoped restricted
bundle/evidence capability between Admission and the assigned Worker. Hidden input, full output,
expected values, and private control receipts do not transit through public CAS or proposal-visible
diagnostics. If that path is unavailable, hidden hardware evidence remains not executed rather than
being downgraded into the public path.

## D-036 — The product crate is explicitly CUDA-to-Ascend-C and concepts do not imply crates

- Decision: accepted

When the new architecture is implemented, `cairn-migration` is directly replaced by
`cairn-cuda-ascend`, including its callers, tests, fixtures, examples, and documentation. Because
Cairn is still pre-release, the old crate name, compatibility re-exports, dual product paths, and
format conversion code are removed rather than retained.

Intent, Oracle, Candidate, Hardware, Feedback, and Knowledge/Skill begin as isolated modules inside
that product crate. A new crate requires evidence such as a process/security boundary, a materially
different dependency set, a second implementation, or two real consumers. Conceptual importance or
hypothetical support for other migration targets is not sufficient.

## D-037 — Admission planning is optional and uses kind-specific typed profiles

- Decision: accepted

There is no universal Admission Planner Agent. Trusted policy mechanically derives the exact
admission-kind-specific required-evidence set before planning. An optional Planner may order checks,
choose among policy-allowed experiments, propose supplemental controls, and explain public receipts;
it cannot delete, downgrade, satisfy, or replace required obligations. Every plan proposal passes a
deterministic typed validator before any external effect.

Intent, Oracle, Hardware Fact, Performance, Candidate, Knowledge, and Skill planning use distinct
profile, applicant, obligation, experiment-request, diagnostic, receipt, and outcome types. They may
share the domain-neutral agent runtime, model provider, and a capability-equivalent Planning Host,
but not private continuation, mutable context, writable artifact namespace, or authority. Intent and
Oracle commonly benefit from reasoning-based planning; Hardware and Performance prefer deterministic
measurement recipes with optional adaptive planning; Candidate Admission initially uses a
deterministic dependency/cost scheduler.

Planner output remains proposal evidence. Mechanical gates run without model transport in the
separate Admission authority process and derive outcomes only from frozen inputs, trusted policy,
authoritative receipts, and qualified mechanisms.

## D-038 — Agent catalog is capability-derived and interaction is artifact-mediated

- Decision: accepted

Cairn distinguishes an Agent-capable product function, a replaceable strategy, a typed planner or
agent profile, a durable episode, a Host process, and an authority. The current design derives eleven
Agent-capable logical positions from four exploration/generation functions and seven Admission Planner
profiles. Eleven is an inventory result, not a protocol constant, process count, concurrency target,
or requirement to invoke a model. Required functions may use deterministic strategies when their
typed contract and policy permit. Blue and Red remain current model-backed Oracle synthesis and
adversarial profiles rather than permanent Agent kinds.

Multiple capability-equivalent episodes may share a Host, but each retains its own identity, context
snapshot, continuation, budget, tool results, writable namespace, and capability grant. Cross-episode
interaction occurs only through immutable provenance-bearing artifacts, typed requests/diagnostics,
and durable events selected by trusted policy. Private continuation, mutable scratch state, unpublished
reasoning, pending tool results, and unsubmitted drafts do not cross episode boundaries. Agent
agreement, voting, or repeated reflection creates neither receipt authority nor stronger evidence.

Processes split when data visibility, external tools, OS sandbox, credentials, or authority differ,
not merely because role names differ. The mechanical Admission Gate remains model-free and outside
every Agent Host.
