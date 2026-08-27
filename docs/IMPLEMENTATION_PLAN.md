# Integrated implementation plan

- Status: active
- Date: 2026-08-27
- Scope: implemented worker/runtime foundation and redesign of the CUDA → Ascend C Oracle program
- Replacement development-plan design: [`dev/README.md`](dev/README.md)

This plan combines the unfinished parts of resource-driven workers and managed enrollment into one
dependency-ordered program. It is an implementation plan, not a replacement for the normative
requirements and system design. A later phase may start only when the preceding phase's acceptance
gate is durable across restart.

## Current checkpoint and next-session entry

Checkpoint commit: `44c1f36` (`feat: materialize executable oracle cases`). The worktree was clean
after that commit. The current implemented frontier is one model-authored, execution-materializable
`matmul-zero-k` `f32` Oracle case:

- Blue emits a strict typed body; trusted code validates the fixed caller shape, ABI order, raw
  vector lengths, numerical-zero semantics, and caller-authorized comparator strength;
- Cairn derives the invocation, empty input buffers, 24-byte proposed reference, canonical input
  bundle, and typed content identities without asking the model to invent identities;
- reference bytes are absent from the candidate process bundle; the existing call-adapter V1 binds
  the typed invocation and captures ABI output through the normal execution port;
- `f32-numeric-exact` retains raw identities but normalizes signed zero before comparison, while
  bit exactness remains available only for a future caller contract that explicitly authorizes the
  zero sign;
- a real host adapter integration test executes this model-shaped input, validates the capture, and
  prepares canonical comparison bytes plus an archival identity;
- the final live-GitHub run converged after one Blue structured-submission repair and three Red
  stability rechecks. It used seven model requests, 128,419 input tokens, 34,024 output tokens, and
  106,240 cache-read tokens. The detailed four-run learning ledger is in `ORACLE_DOGFOOD.md`.

**Architecture reset (2026-08-27):** implementation is intentionally paused at this frontier. The
fixed `matmul-zero-k` path is now classified as a transport/materialization control, not the template
for the final Oracle-generation product. Adding a nonzero-K companion would improve this control but
would not resolve semantic authority, comparator justification, coverage, real device evidence, or
performance. It is therefore no longer the automatic next implementation task.

Before implementation resumes, Phase G must be re-sliced against the refreshed
[`SYSTEM_DESIGN.md`](SYSTEM_DESIGN.md) and the focused designs for
[code organization](design/CODE_ORGANIZATION.md),
[logical architecture](design/LOGICAL_ARCHITECTURE.md),
[runtime architecture](design/RUNTIME_ARCHITECTURE.md),
[Admission architecture](design/ADMISSION_ARCHITECTURE.md),
[Agent architecture](design/AGENT_ARCHITECTURE.md),
[semantic-intent recovery](oracle/SEMANTIC_INTENT_RECOVERY_DESIGN.md),
[Oracle exploration](oracle/ORACLE_EXPLORATION_SYSTEM_DESIGN.md),
[independent admission](oracle/INDEPENDENT_ADMISSION_DESIGN.md),
[performance/hardware](oracle/PERFORMANCE_ORACLE_DESIGN.md), and
[knowledge/skill trust](oracle/KNOWLEDGE_AND_SKILL_TRUST_DESIGN.md). The replacement development stages,
dependencies, slices, entry/exit gates, and workstreams are now defined under
[`dev/README.md`](dev/README.md); they remain plan design and do not authorize code work. The first new
slice must establish
the proposal-only `Semantic Intent Recovery` boundary and separate Intent Admission for one kernel;
it must not let the extractor directly produce an admitted migration contract. Implementation may
resume only after the relevant `docs/dev/` entry gate is reviewed and the named P0 blockers close.

Full G13 remains incomplete until production intent/proposal/revision/attack/admission artifacts
consume the material, generic durable `AgentEpisode` owns the loop budgets, candidate-specific CUDA
and Ascend C adapters execute the resulting portfolio on the declared paths, and performance is
admitted against scoped hardware facts and workloads. Scarce GPU/NPU work remains deferred until an
applicable proposal passes cheaper gates.

Every replacement Phase G slice must first produce the `DesignConformanceRecord` required by
[`oracle/DESIGN_INVARIANTS.md`](oracle/DESIGN_INVARIANTS.md), including authority/capability boundaries,
required claims, feedback/hidden-corpus use, mechanism qualification, controls, receipt closure,
strong-type obligations, and explicit unknown/not-executed scope.

Carry these constraints into the next session:

- modify current V1 definitions directly; add no compatibility readers, migrations, aliases, or
  internal version increments during pre-release development;
- preserve necessary strong types at trust and identity boundaries without generalizing the first
  slice into a speculative universal operator schema;
- keep Cairn's product scope strictly CUDA → Ascend C even where the agent, record, and worker
  infrastructure remains domain-neutral;
- keep Semantic Intent Recovery isolated and proposal-only so its implementation can be replaced or
  optimized without changing admitted contracts or judge authority;
- when the current model-backed Blue/Red synthesis/adversarial profiles are selected, they keep
  distinct durable episodes/continuations; other policy-selected strategies remain possible, and
  model agreement never substitutes for trusted claim-strength or admission policy;
- treat previous-iteration and real-model feedback as typed evidence, never as an untyped reward or
  an in-place mutation of an admitted artifact;
- keep performance as an independent gate using conditional, measured hardware ceilings; it cannot
  compensate for correctness failure;
- preserve knowledge/skill trust state, exact content identity, allowed use, and retraction impact;
- logging remains observational: no fallible, async, stateful, or business-semantic work belongs in
  tracing event fields or span lifetimes;
- retrieved upstream tests are untrusted research context, not corpus or semantic authority.

## Ordering principle

Scheduling must never place work on an identity whose authority is ambiguous. Resource discovery
must not smuggle product roles into a worker profile. One-command onboarding must compose the same
credential, profile, and configuration boundaries used by the lower-level commands. Therefore the
order is authority, rotation, scheduling, richer probes, registry transition, then onboarding UX.

## Phase A — credential authority foundation (implemented 2026-08-25)

Separate the stable logical principal from the credential used to prove it. Persist `CredentialId`
in registration/session facts, and persist independent authority facts for credential revocation,
logical-worker disablement, and unused-enrollment revocation. Authentication must reject an
inactive authority before registration. An already connected managed worker must be disconnected
once the controller observes revocation or disablement.

Acceptance gate:

- one `WorkerId` remains bound to one stable authentication subject and pool while successive
  incarnations may use different `CredentialId` values;
- one live incarnation cannot silently change credentials;
- revoked credentials and disabled workers cannot create a registration fact;
- an unused revoked `EnrollmentId` cannot issue a credential;
- authority decisions survive controller restart and concurrent administrative SQLite access;
- the managed registry is the only runtime authority; controller configuration has no static
  enrollment or import path.

## Phase B — safe credential rotation (implemented 2026-08-25)

Add a rotation offer bound to an existing `WorkerId` and current authority, rather than allocating a
new worker. Issue the new certificate to a fresh worker-local key/CSR while retaining the existing
pool. Model `issued`, `active`, `superseded`, `revoked`, and `expired` credential outcomes without
turning certificate expiry into implicit logical-worker deletion. Support a configurable overlap
window, atomic local credential swap, rollback before cutover, and explicit retirement of the old
credential. Numeric timing policy must be configurable or explicitly disabled where safe.

Acceptance gate:

- rotation preserves `WorkerId`, stable principal, pool, and scheduling history;
- both credentials work only during the configured overlap, and retirement/revocation closes the
  old one immediately at the application authority boundary;
- response loss and restart recover the exact rotation issuance;
- a failed local swap leaves one usable credential and no half-written managed state;
- no CRL/OCSP dependency is required for Cairn's baseline, while an external issuer adapter may add
  either later.

## Phase C1 — scheduler reservation kernel (implemented 2026-08-25)

Introduce the generic scheduler between migration requests and worker assignment. It consumes a
frozen `PlacementRequest` plus a consistent candidate snapshot, filters by pool/platform/backend/
capability/authority/liveness/capacity, applies a deterministic versioned policy, and durably
reserves capacity before granting a lease. The V1 global ledger deliberately serializes placement
reservation facts through SQLite optimistic concurrency: this is a correctness baseline that may
later be sharded without changing `PlacementId`, `ReservationId`, or snapshot semantics.

Acceptance gate:

- multiple candidates produce one deterministic, explainable choice;
- concurrent placement cannot overcommit slots;
- stale snapshots and authority changes fail closed;
- replay reconstructs the candidate set, policy version, rejection reasons, reservation, and choice.

## Phase C2 — scheduler composition and migration translation (implemented 2026-08-25)

Compose the scheduler authority seam with the managed enrollment registry and the controller's
assignment/outbox path. `cairn-migration` owns translation from migration-stage needs to the frozen
domain-neutral request; workers never receive business roles. Configuration selects the supported
policy version and supplies positive session, reservation-claim, and lease timing values.

Acceptance gate:

- a registry revision is cited in every managed candidate authority observation;
- a revocation between snapshot and assignment grant fails closed in the composed controller;
- reservation release follows pre-start expiry or terminal execution but never an in-doubt start;
- one migration fixture reaches an assigned worker without product vocabulary in execution types.

## Phase D1 — typed startup resource observation (implemented 2026-08-25)

Extend the built-in platform probe beyond architecture/OS/target environment to typed quantitative
inventory: logical CPU, memory, local scratch space, accelerators, and backend/device capabilities.
Every observation retains source, timestamp/freshness, and probe version. Configuration supplies
expected constraints, never forged observed values. The first observation is immutable for one
worker incarnation. Its optional expiry is configuration; disabling expiry keeps it valid for that
incarnation. The generic matcher evaluates typed CPU, byte, accelerator-count, discovery-
completeness, and per-device capability requirements.

Acceptance gate:

- probe fixtures cover x86-64 and AArch64 plus absent/partial accelerator discovery;
- overflow, unit mismatch, duplicate device, stale evidence, and expected-value mismatch fail closed;
- frozen contracts and profiles use V1 identity domains and reject non-V1 bytes rather than
  assigning invented quantitative meaning;
- matching remains vendor/product neutral and counts only accelerator devices satisfying every
  requested device capability.

## Phase D2 — refresh, quantitative reservation, and trusted admission (implemented 2026-08-25)

Add a resource-update protocol independent of immutable worker-profile identity. Freeze each
scheduler snapshot against an exact admitted observation revision, subtract outstanding
quantitative reservations, and recheck that revision at assignment grant. Refresh/admission
intervals and thresholds remain configurable or disableable. A controller challenge or external
attestation may supersede built-in observations only through a typed admission fact; worker hello
cannot self-assert higher provenance. Trusted admission is currently an on-demand port; any future
periodic verifier adapter must expose its interval and policy in configuration.

Acceptance gate:

- refresh survives reconnect/restart without mutating a historical worker profile;
- scheduler reservations consume CPU, memory, scratch, and matching accelerator inventory without
  embedding vendor/product roles;
- concurrent placements cannot overcommit any quantitative dimension;
- stale or superseded observation revisions fail assignment grant;
- externally attested or controller-verified claims can replace a built-in claim through a typed
  admission seam.

## Phase E — registry and operator lifecycle closure (implemented 2026-08-25)

Use the persistent enrollment registry as the only worker-credential authority from first startup.
The controller has no static certificate list or import path. List/show/audit commands, worker
re-enable, credential inspection, and separate pool reassignment keep worker, credential,
enrollment, and pool histories distinct.

E2a implements the mutation side of operator lifecycle. Revoke, disable, re-enable, and pool
assignment require explicit strong `CommandId` values and recover the original fact on exact retry.
Pool is an independent worker projection: reassignment requires the worker to be disabled and an
actual pool change. A subsequent handshake reloads current registry authority rather than a startup
snapshot, then cross-links the exact pool-assignment event into the execution-worker stream. That
cross-link can modify only a durably disconnected or exactly expired session.

E2b implements the read-only operator surface. `registry list`, `show-worker`, `show-credential`,
and `audit` rebuild the complete causal history before emitting versioned strict JSON. Reports use
strong worker/credential/event identities, retain pool authority revisions plus rotation
lineage, distinguish active/worker-disabled/retired/revoked credential states at an explicit
observation time, and omit secrets, certificate bytes, and unstable paths. Invalid history fails
without a partial report.

Acceptance gate:

- a static deployment can be imported without changing `WorkerId` or losing provenance;
- ordinary startup no longer needs a copied certificate list;
- every lifecycle command is idempotent under an explicit command identity and produces an
  auditable fact;
- revoke, disable/re-enable, pool change, and import survive restart and reject contradictory
  histories.

## Phase F — one-command join and real worker execution

Status: F1 join/bootstrap, F2 material transfer, and F2 Docker execution are implemented. F2 has
been measured with a real local Docker daemon.

F1 composes enrollment, the built-in host/resource probe, fixed local state, and strict worker
configuration behind `cairn-worker join <bundle> <state-dir>`. The generated worker stays
unavailable and draining until an operator explicitly enables an execution backend.

F2 has one deliberately narrow responsibility: carry exact input/environment objects to the
selected worker, execute one Docker attempt, durably publish its result, and recover that same
attempt after a worker restart. It consists of:

1. a typed manifest and bounded resumable transfer into worker-local SQLite/CAS;
2. strict canonical input-bundle and Docker-environment artifacts;
3. an explicitly activated `docker-v1` adapter using an immutable full image ID;
4. deterministic `AttemptId` container identity and reconciliation of absent, created, running,
   and exited states;
5. configurable or disableable CPU, memory, PID, writable-work, material, timeout, and capture
   bounds;
6. terminal journal/outbox commit before container and attempt-directory cleanup.

The project runs in operator-controlled private infrastructure. Submitted code and images are the
operator's responsibility. Docker is used for reproducible packaging and restart-visible process
state; F2 does not attempt hostile multi-tenant isolation, malware detection, arbitrary runtime
abstraction, or a Kubernetes-like control plane.

F2 acceptance evidence:

- ordinary SQLite tests reopen a worker journal after the start fact and recover exactly one
  execution authority; after terminal publication they recover none;
- `scripts/docker-hello-smoke.sh <full-image-id>` runs a real container, captures `hello world`
  plus a declared artifact, and requires byte-identical capture when the exited attempt is replayed;
- worker activation remains one coherent configuration invariant and join remains disabled;
- cleanup happens only after terminal observation and outbox publication are durable.

Service-unit generation, accelerator device exposure, additional network modes, multiple concurrent
attempts per worker, and stronger container isolation are not part of F2. They may be added as
small, demand-driven slices once the CUDA-to-Ascend migration workflow requires them.

All worker-control, journal, content, and configuration formats remain schema V1. During pre-release
development, incompatible changes replace V1 directly and development state is rebuilt; there are
no conversion or compatibility branches.

## Phase G — executed oracle admission (implementation ledger; redesign pending)

> **Planning status (2026-08-27):** the entries below remain the factual ledger for work already
> implemented and for the old narrow plan. Their unfinished ordering is suspended and is not the
> approved next-step sequence. A replacement Phase G plan must be derived from the refreshed intent,
> Oracle, performance, feedback, and knowledge/skill designs before code work resumes. That replacement
> sequencing is now maintained in [`dev/ROADMAP.md`](dev/ROADMAP.md) and
> [`dev/SLICE_CATALOG.md`](dev/SLICE_CATALOG.md); this section remains a historical implementation ledger.

The next product milestone is M2, not a wider worker runtime. Its first implemented foundation is a
new domain-neutral `cairn-verification` crate with strict V1 admission-policy and numerical-allowance
contracts. Policy owns variant minima, required construction/fault classes, structural independence,
saturation, accepted strengths, execution scope, and budget-exhaustion outcome. Numerical provenance
and assurance are separate; held-out derivation and validation corpora must be identity-disjoint;
asserted and external-prior-only values cannot support a pass; held-out evidence is empirical at
most; and only proven/exhaustive evidence may support an unqualified domain-wide numerical claim.

The historical G slices and their recorded state are:

1. **Implemented 2026-08-25:** immutable caller-domain, refinement, corpus-proposal, authorship,
   construction-claim, correct/wrong-variant, and oracle-proposal manifests. Proposal schema cannot
   carry trusted policy, allowance, mutant, comparison, or decision fields, and semantically
   different evidence categories have different content identity domains.
2. **In progress:** the strongly typed operator-domain V1 body and trusted quantitative boundary
   derivation now cover ABI buffer/parameter roles, dtypes, symbolic shapes/ranges, invalid behavior,
   min/max/zero/one/interiors, invalid neighbors, first/last tile tails, dtype extrema, signed zero,
   non-finite/subnormal inputs, cancellation, mixed finite scales, null/misaligned pointers,
   insufficient capacity, exact aliasing, and applicable partial overlap. Remaining mandatory work
   includes populating/reproducing historical fixtures and completing executable corpus
   orchestration. Exact dtype recipes now materialize into caller-bounded deterministic bytes with
   typed element/byte quantities, checked overflow/allocation, and source/byte content identities;
   trusted quantitative boundary cases now rederive exact domain membership, resolve buffer shapes,
   encode scalar ABI files, describe output allocation lengths, and emit canonical ABI-ordered
   `InputBundleV1` artifacts with cross-validated manifest/source/file identities. Separate
   supported and explicitly-invalid dtype obligations now share one manifest/assembly path, require
   a trusted successful quantitative baseline, vary exactly one input buffer, bind that target's
   materialization identity, and derive success or the caller-declared invalid outcome directly from
   the obligation. The adapter-neutral part of memory-surface realization is also implemented: an
   executable obligation requires a trusted successful quantitative baseline and supported input
   bytes, then emits a distinct manifest with one exact null, misalignment, capacity-shortfall,
   exact-alias, or partial-overlap layout. Required/accessible lengths, alignment/offset quantities,
   ABI positions, and shared allocation extent are cross-validated against baseline arguments;
   unknown and excluded conditions are rejected. Vendor-specific pointer construction and real
   call-adapter execution are still pending. The isolated
   process input boundary is now implemented for all three assembled case categories: caller-bounded
   executable bytes receive a separate content identity, a strict V1 request binds the exact source
   bundle and typed case-manifest identity, and the final canonical bundle carries the executable
   bit plus a fixed no-shell command using Cairn input/work/output roots. The result/observation side
   is now implemented as a strict V1 adapter report. Successful cases freeze output-capable ABI
   arguments, paths, and exact lengths in the request; captures bind the request and typed invocation
   identity plus every raw-output identity. Invalid cases capture only completion because their
   output buffers are unspecified. Reject-before-invocation, void return, and typed status return
   remain distinct, and a required error status is checked exactly. Generic job composition is now
   implemented: caller-supplied
   stream/result/diagnostic/evidence limits, exact adapter input identity, environment identity,
   no-shell command, disabled network, translated placement/resources, and declared outputs produce
   canonical `JobContract` bytes and identity ready for normal execution preparation. The migration
   validation tier stays only in the product wrapper and is absent from worker-facing bytes. Real
   vendor adapters remain pending. Authoritative receipts returned by generic execution completion
   or recovery can now be ingested with their typed receipt identity: migration validation checks
   the exact job/contract/input/command/output-declaration binding, requires a successful terminal
   outcome, reads every declared result through the typed content store, and then reuses strict
   adapter-result/ABI-byte validation. Content archival without the generic execution fact does not
   confer authority. A deterministic Rust host fixture now supplies the first executable transport
   gate: a real fixture process validates the typed invocation identity, emits bounded declared
   outputs, runs through generic execution start/completion and durable recovery, and is finally
   admitted as the exact receipt-bound observation. Tampered invocation bytes are rejected before a
   result is written. A canonical complete-corpus execution plan now commits all three mandatory-set
   roots, their shared caller domain, the exact source/reference/property/admission-variant/candidate
   subject role and upstream artifact, the adapter executable, validation tier, and one independently
   identified generic job per executable obligation. The plan derives its executable subset from
   typed dispositions, orders it canonically, and rejects missing, duplicate, extra, reordered,
   cross-domain, expectation-changed, or unknown-role material. Subject role and identity participate
   in plan identity, so a candidate run cannot later be relabeled as a reference run. Unknown and
   explicitly excluded
   obligations remain visible through their set roots without becoming executable jobs. Complete
   receipt collection is now also implemented: unordered authoritative receipts are matched by the
   planned `JobId`, then every boundary, dtype, or memory case passes its category-specific exact
   receipt/result/output validator before a strict V1 observation set can bind the plan, receipts,
   and result identities. Missing, duplicate, extra, crossed, non-successful, or content-incomplete
   receipts fail closed. The observation set carries no semantic verdict. This fixture, plan, and
   collector do not substitute for CUDA/Ascend C adapters, device isolation, complete-plan dispatch,
   general numerical/property comparison, or oracle admission. The first exact-only comparison slice
   is implemented for caller domains whose semantic claim is `Exact`: it requires a `Reference` plan
   and either a `Candidate` or `AdmissionVariant` subject plan over the same domain and mandatory
   roots, validates both observation-set identities, aligns typed obligations, and records paired
   completion values and exact ABI-output identities. Per-output, per-case, and full-set matches are
   recomputed rather than trusted as persisted booleans. The strict comparison artifact rejects
   verdict fields and remains evidence, not reference admission or trusted adjudication.
   Strict historical record/obligation/coverage contracts now bind provenance, target-oracle scope,
   observed stage, required detector, exact record identity, domain family, and caller-domain
   identity;
3. **Implemented 2026-08-26:** versioned trusted-mutant sets, complete Cartesian mutation-grid
   trials, and recomputed proof obligations. The admission policy now selects one exact
   content-addressed mutant set. Every grid binds that policy, the tested implementation, frozen
   admission corpus, canonical mutant and case axes, and exactly one trial per cell; missing,
   duplicated, reordered, extra, or cross-axis cells fail closed. Applied trials retain separately
   typed injection, execution, and comparison evidence plus their exact execution scopes. One batch
   evidence artifact may support multiple cells. Policy-sized and scale-free misses become
   fatal proof failures, case-dependent misses become mandatory blind spots, explicit
   non-injectability remains visible, required fault-class coverage is recomputed, an empty
   applicable grid fails, and comparator-only trials fail the implementation-path obligation. The
   proof has no stored `passed` field and validates only by recomputation against its exact policy,
   mutant set, and grid. The historical-reduction adapter now supplies the first actual composition:
   its closed drop-last, unit-offset, and zero-output kinds bind the trusted definition to the exact
   wrong variant, fault evidence, implementation, authoritative build, and real admission-variant
   run. It derives mutation-case identities from the frozen case bodies, recomputes every reference
   versus mutant ULP distance, and derives detected/missed without accepting either a verdict flag or
   opaque evidence identity from the caller. Strict V1 injection and comparison artifacts reject
   kind/algorithm mismatches, changed ULP facts, unknown fields, and non-V1 input. The historical
   control and admission APIs accept only the privately constructed product preparation, so an
   arbitrary caller-authored generic grid cannot bypass this composition;
4. **In progress:** exact host execution-port composition now binds correct and deliberately wrong
   variants to implementation bytes, fixed build jobs, authoritative generic receipts, exact built
   adapter identities, complete admission-variant plans, observation sets, comparisons, and
   recomputed `MustAccept`/`MustReject` expectations. The fixture directly executes the build driver
   and representative built adapters, while deterministic generic captures keep the complete host
   corpus gate bounded. It emits no admission verdict. Real compiler/vendor/device adapters and
   production numerical/property variants remain pending;
5. **Implemented 2026-08-26:** the hardware-free
   historical reduction control loads an ordinary proposal graph, builds and executes two distinct
   correct and three distinct wrong compiled host implementations through generic authoritative
   receipts, stores finite-f32 bits rather than JSON floats, and recomputes every per-case ULP
   distance. The archived single sample derives the old zero-ULP allowance and rejects the correct
   balanced tree; measured family spread derives one ULP and accepts both correct variants while all
   wrong variants are red. The executed 3-by-2 mutation grid reuses those authoritative wrong runs:
   scale-free unit-offset and zero-output cells are detected, while a trailing-zero case honestly
   exposes one case-dependent drop-last miss. The selected mutation proof must retain that blind
   spot and mutate one of the correct implementations. Asserted allowance, an empty applicable
   grid, mutant/algorithm relabeling, changed content, stored `passed`, and non-V1 input fail closed;
6. **Historical admitted-oracle and candidate-verdict receipts implemented 2026-08-26:** the reduction
   control now recomputes all product evidence before emitting a frozen admitted-domain manifest,
   complete admission receipt, and immutable `AdmittedOracle`. The graph binds proposal, task,
   policy, empirical reference strength, corpus, measured allowance, host environment, source
   observation, correct/wrong trials, mutant set/grid/proof, two no-new-class saturation rounds,
   historical coverage, blind spots, assumptions, explicit unverified target/device claims, and
   revalidation triggers. Receipt/oracle identities and mirrored edges are recomputed; missing
   saturation, changed frozen corpus, stored `passed`, unknown fields, and non-V1 schemas fail
   closed. A candidate-specific execution role now binds the implementation, authoritative build,
   environment, frozen corpus, and run receipt. Product comparison recomputes exact reference and
   candidate f32 bits plus every ULP distance; the domain-neutral receipt cites source/build/run and
   comparison evidence, derives `Pass` only from an empty failed-case set, and carries the oracle's
   blind spots, assumptions, and unverified claims forward. The balanced-tree control candidate
   passes, zero-output fails with exact case identities, and relabeled admission-variant evidence is
   rejected. General candidate-search integration and target-device verdicts remain pending.

Phase G now has a complete hardware-free admission-to-candidate-verdict control, but remains
incomplete while step 4 lacks production compiler/vendor/device adapters and general numerical and
property variants. Open questions OQ-004 and OQ-007 remain
unresolved; implementation must preserve derivation and disagreement evidence rather than selecting
an independence or automatic disagreement policy.

### Phase G7–G13 — model-authored Oracle Agent and dogfood

These slices close the gap between the fixed historical proposal control and a real OracleSearch.
They are ordered so that paid/live model work begins only after its cache cost and external research
are observable. The focused contract is [`ORACLE_AGENT.md`](ORACLE_AGENT.md).

#### G7 — cache usage and stable-prefix evidence

Status: implemented 2026-08-26.

1. Replace the current V1 provider-usage body with one that can retain optional provider-reported
   cache-read, cache-write, and cache-miss token counts alongside total input/output counts.
2. Parse the supported Responses, Chat/DeepSeek, and Anthropic usage shapes at the protocol-aware
   transport boundary without inferring missing values from bytes or a local tokenizer.
3. Keep episode token budgets based on the validated total input/output counts; cache details are
   attribution observations and cannot change authority.
4. Add deterministic role-prefix artifacts whose ordering is common instructions, role
   instructions, caller/source context, policy, append-only evidence, then current diagnostics.
5. Prove strict V1 round trips, missing-detail behavior, overflow rejection, response/event
   retention, and byte-stable reconstruction.

Acceptance: offline per-protocol usage fixtures and a two-turn prefix control pass. No live provider
claim is required.

Implemented evidence: protocol-aware usage fixtures retain optional cache read/write/miss counts;
durable response recovery preserves them; `OracleRolePromptV1` reconstructs the same role
instructions, tool schema, initial input, and second-turn native request across a CAS restart.

#### G8 — OracleSearch plan and role isolation

Status: implemented 2026-08-26.

1. Add a strict `OracleSearchPlanV1` in `cairn-migration` binding task, caller domain, source/task
   inputs, admission policy, shared immutable context, and exactly one blue and one red episode.
2. Require distinct episode identities, frozen model configurations, role-specific tool catalogs,
   budgets, and visibility roots.
3. Define closed product roles and role tool policies while keeping `cairn-agent` domain-neutral.
4. Prove blue/red private-history non-sharing and reject swapped, duplicated, or cross-task
   bindings.

Acceptance: canonical plan/identity mutation suite and capability matrix pass.

Implemented evidence: `OracleSearchPlanV1` binds separate episode/model/authorship/budget/tool
edges for Blue and Red, and strict tests reject shared sessions, swapped roles, private-context
leaks, changed identities, unknown fields, and non-V1 input.

#### G9 — bounded external-test research

Status: implemented 2026-08-26.

1. Register `oracle_search_external_tests` for blue as `ReadOnly` with a pinned implementation
   version.
2. Define bounded strict request/result contracts for query, approved repository scopes, maximum
   results, exact upstream path/blob/revision, fetched bytes or excerpt, retrieval provenance,
   and truncation.
3. Implement a replaceable provider port, an offline recorded provider, and a live GitHub adapter
   whose endpoint, credentials, redirects, repositories, and response sizes are trusted
   configuration rather than model arguments.
4. Validate the typed result before it becomes model-visible. Search snippets alone never become
   executable corpus material.
5. Archive research source and retrieval provenance without creating `CorpusCaseArtifact`; require
   Blue to author a separate structured Cairn case that may cite the research-result identity.

Acceptance: a recorded PyTorch-like result traverses the normal durable tool operation; query,
scope, blob, and byte mutations fail; research bytes have no typed corpus-promotion path. The live
adapter has an opt-in test only.

Implemented evidence: the recorded PyTorch-like GitHub search traverses the normal durable
read-only operation lifecycle; exact source, blob, retrieval, and provenance artifacts are archived
separately from executable corpus cases. The fixed-authority live adapter has an ignored opt-in test
and does not spend a request on repository-license lookup.

#### G10 — Blue proposal episode

Status: implemented 2026-08-26.

1. Project the minimum structured caller contract, source snapshot, mandatory trusted cases,
   historical obligations, and blue tool catalog into a durable blue episode.
2. Accept typed model submissions for refinements, reference/property proposals, corpus additions,
   valid-family plans, correct variants, and the aggregate `OracleProposalV1`.
3. Require model authorship to cite the exact episode and resolved model configuration.
4. Keep external research results as evidence-citing proposals and preserve caller declarations and
   explicit unknowns unchanged.

Acceptance: a recorded model uses external research and submits one complete proposal; missing
evidence, wrong episode/model identity, or caller-domain rewriting fails.

Implemented evidence: the Blue native catalog includes bounded research and typed submissions;
`BlueProposalGateway` accepts a complete canonical `OracleProposalV1`; the immutable revision
boundary requires the exact task inputs, unchanged caller-domain identity, Blue episode, Blue model
configuration, and canonical research citations.

#### G11 — Red attack episode and trusted feedback

Status: implemented 2026-08-26.

1. Open a separate red episode over the frozen public contract of one proposal.
2. Accept correct-by-construction and deliberately wrong variants plus adversarial cases through
   role-specific typed tools.
3. Execute all already-authorized cheaper admission diagnostics before buying a correction turn.
4. Emit a typed diagnostic bundle identifying responsible role, proposal revision, evidence,
   disagreements, missing obligations, false accepts/rejects, and infrastructure-only blocks.
5. Feed only submitted red artifacts and trusted diagnostics to blue; never copy red native history.

Acceptance: capability escape attempts fail and one rejected proposal produces a linked immutable
revision rather than mutation.

Implemented evidence: `OracleAttackV1` requires model-authored Red correct and wrong variants over
one frozen Blue revision. `OracleAdmissionFeedbackV1` binds the exact attempt and typed evidence to
Blue, Red, or both; a correction must create a changed child revision and cannot mutate or repeat
its parent.

#### G12 — first model-authored admitted oracle

Status: implemented 2026-08-26 for the hardware-free control.

1. Drive the historical reduction input through the same model-authored proposal boundary, using
   recorded providers in ordinary CI and an opt-in live provider gate.
2. Reuse the existing executed correct/wrong variants, mutation grid, measured-family allowance,
   blind spot, saturation, and immutable admission receipt.
3. Produce an `AdmittedOracle` that explicitly retains unverified Ascend device claims and NPU
   revalidation triggers.
4. Prove that this oracle can judge the existing host candidate control but cannot claim a target
   device verdict.

Acceptance: the full hardware-free model/tool replay is deterministic over recorded external
outcomes and has a complete evidence graph. The typed control closes G12, but M3 does not begin
until the live/recorded product loop in G13 has eaten enough dogfood to prove that real model output
can traverse it.

Implemented evidence: the historical reduction integration now creates the ordinary proposal with
Blue model authorship, creates all two correct and three wrong controls with Red model authorship,
passes both through revision/attack validation, and then reuses the complete executed admission
graph. Its admitted oracle judges the existing host candidates while retaining target-device
behavior as unverified and omitting `TargetDevice` from executed scopes.

This evidence validates the model-authorship contract but its proposal and variants are constructed
by recorded test code, not emitted by a live model. It must not be cited as full Oracle Agent
dogfood.

#### G13 — full Oracle Agent dogfood

Status: active 2026-08-26; live Blue research, repository-owned prompt contract, atomic
self-correction, and bounded Blue/Red draft debate complete; full proposal/admission loop
incomplete.

1. Put Blue and Red limits for turns, logical tool operations, cumulative provider tokens, and
   output tokens per turn in strict configuration. Configure external research provider,
   repository allowlist, result count, response bytes, and credential-file reference separately.
2. Run real Blue model dispatch through the production-native catalog, durable external-research
   operation, continuation restart, and cache metering. Keep a recorded research counterpart for
   ordinary CI and add opt-in live GitHub research.
3. Materialize independently authored model drafts and their referenced bodies before accepting a
   domain refinement or corpus proposal; never ask a model to invent a content ID for content that
   Cairn has not archived.
4. Drive Blue and Red through generic durable `AgentEpisode` coordination so configured budgets are
   enforced by durable facts rather than merely frozen in the search plan.
5. Complete Blue proposal, separate Red correct/wrong/adversarial attack, trusted diagnostics,
   immutable Blue revision, and hardware-free admission using actual model tool calls.
6. Repeat multi-turn runs and compare provider-reported cache reads, uncached input, total cost, and
   task quality without using cache hits as correctness evidence.
7. Treat exact upstream bytes and bounded model context separately. Select line-addressed snippets
   deterministically from query matches and retain the full blob and result identities outside the
   prompt.
8. Dogfood a semantic matrix covering identity, rejection, layout, special values, and zero-work
   behavior. Freeze a typed Blue draft, then let an isolated Red episode inspect only shared
   contracts, the frozen draft, and its cited bounded evidence. Red must classify findings as
   blockers or advisories; repeated verdict disagreement cannot pass admission.
9. Keep repository-owned common and role instructions as content-addressed stable prefixes. Treat
   retrieved content as data without instruction authority and test exact corrective feedback in
   the producing role's continuation.
10. Run Red blockers through a bounded artifact-mediated loop: changed complete Blue revision,
    immutable identity, Red re-review, and focused blocker-free stability rechecks. Record explicit
    non-convergence on exhaustion rather than voting or passing by timeout.

Acceptance: both recorded and live-model paths traverse the same product gateways; one opt-in run
uses live GitHub; every advertised tool has an executable gateway; restart and budget controls are
durable; fetched upstream bytes never become corpus artifacts; and the final hardware-free oracle
has a complete reconstructable Blue/Red/admission graph.

Implemented evidence so far: the first real DeepSeek Blue run exposed and led to fixes for a
missing tool-catalog CAS write, provider-incompatible dotted tool names, and discarded transport
diagnostics. After those fixes, a two-turn run selected the bounded research tool, executed a
recorded PyTorch result through the durable read-only gateway, reconstructed a byte-identical
continuation after restart, and reported 896 cache-read tokens on the second turn. The detailed
ledger and GitHub credential contract are in `ORACLE_DOGFOOD.md`. Live GitHub dogfood then exposed
an 82,287-token full-file prompt, a vacuous empty-reduction proposal, provider output split across
multiple semantic text items, and insufficient 2k/8k reasoning-output limits. V1 now sends at most
4 KiB of query-centered source per result while archiving the full blob; the same run fell to 3,112
input tokens. Five typed Blue samples and isolated Red reviews now exercise empty-sum identity,
empty-max rejection, non-contiguous strides, NaN propagation, and zero-K matmul. Exact PyTorch test
identifiers improved evidence retrieval; blocker/advisory typing closes internally inconsistent Red
passes. The live harness now uses audited repository-owned Blue/Red prompts, rejects malformed or
cross-field-invalid submissions atomically, returns the exact diagnostic in the same role
continuation, rejects byte-identical revisions, and carries isolated Blue/Red continuations through
up to six revision rounds plus three focused pass rechecks. Opt-in limits have been raised to 64
turns, 128 logical tools, 4,000,000 cumulative provider tokens, and 131,072 output tokens per role
turn for complex cases. These are ceilings; the generic durable episode, iterative multi-search,
full `OracleProposalV1`, revision, attack variants, and trusted admission graph remain unfinished.
Post-audit live runs over non-contiguous sum, empty-axis max, and NaN sum made 6, 9, and 6 provider
requests respectively. Empty-axis max exercised a real Red-blocker/changed-Blue-revision round;
all three then completed three blocker-free stability rechecks. One Red response used 14,868 output
tokens, exceeding the former 8k ceiling and providing direct evidence for the enlarged headroom.
The first run after structured logging covered zero-K matmul in six provider requests with no
submission repair or Red blocker. It produced the correct semantic result, but a downstream audit
failed it explicitly because the dogfood draft still lacks typed ABI arguments, materialized bytes,
an archived call adapter, and comparison evidence. The gate reported this separately from debate
convergence and made materialization through the existing execution ports the next gate. The
current V1 implementation now requires a model-authored typed zero-K `f32` body, revalidates the
fixed caller sample and claim strength, materializes little-endian input/reference bytes and typed
identities, excludes reference bytes from the candidate bundle, and binds the invocation to the
existing call-adapter protocol. Numeric exact comparison normalizes signed zero while retaining raw
content identities. Integration controls launch the real host fixture and prove capture validation,
`+0/-0` equivalence under numeric exact, and inequality under bit exact. Four live iterations then
exposed and fixed the signed-zero authority boundary: Red disagreement is not trusted admission
policy. The final run repaired one typed ABI-index error, converged without a Blue/Red revision,
completed three stability rechecks, and materialized a downstream-ready single case. G13 remains
incomplete until the production proposal/admission graph consumes this material and a nonzero-K
companion prevents an unconditional-zero false accept.

The real execution deployment prerequisite is now in place: managed V1 workers on the AArch64 GB10
and x86-64 Ascend hosts are durably registered and heartbeating through isolated reverse tunnels.
The `docker-v1` adapter now has a closed typed accelerator policy for no device, one indexed NVIDIA
device, or one indexed Ascend device with fixed manager/driver bindings. The next production-adapter
gate is operational: activate the dedicated GB10, wait for one genuinely free shared Ascend device,
and prove device-visible CUDA and Ascend containers before scheduling migration work to either pool.

The GB10 half of that gate is complete: the live scheduler selected the managed ready worker twice
consecutively, device-visible execution returned a terminal receipt with trusted NVIDIA-device
evidence, and both reservations were safely released. ACK-only control frames no longer acknowledge
one another, removing the live outbox write loop found by this gate. The Ascend half remains gated
only by shared-host capacity: all seven devices were occupied at the observation point, so its
current worker correctly remains unavailable/draining.

The source reference has now crossed that same boundary. Cairn archived the exact five-file
Alloyport `cuda-reduction-v1` intake plus a fixed `sm_121` build adapter, selected the capability-
matched GB10 worker, compiled and linked the CUDA implementation inside the immutable CUDA 13
image, and executed all nine release cases twice consecutively. Both authoritative receipts carry
the exact expected input checksum and trusted NVIDIA device-0 evidence, and both terminal
reservations were released. This completes real source-side reference execution; target-side Ascend
build and device execution remain the next production-adapter work.

The target toolchain prerequisite is also complete without waiting for a shared card. A distinct
managed `npu-build` identity advertises exact build-role, Ascend, CANN 9.1.0-beta.1, and dav-3510
capabilities while its closed Docker accelerator policy is `none`. Twice consecutively, the live
scheduler sent the same content-addressed Alloyport `ascend-add-v1` source/header/CMake bundle to
that worker; CMake selected `bisheng`, compiled the Ascend C source, linked `libadd_custom.a`, and
returned a terminal receipt with trusted `docker:accelerator:none` evidence. This is a real CANN
toolchain gate, not reduction-candidate build evidence or device correctness. The device worker
remains unavailable/draining until a shared card is genuinely free.

## Cross-cutting O1 — structured operational logging

Status: implemented foundation 2026-08-26; broader product/API metrics remain active work.

The initial audit found no subscriber and only sparse free-form server/worker stderr messages. The
workspace now has a shared strict JSON/compact stderr initializer and structured lifecycle events
for model dispatch, tool operations, agent episode opening/completion, Oracle debate, scheduling,
worker sessions, assignment transfer, local execution, and controller reconciliation. Events cite
typed correlation identities, usage/counts, elapsed time, and outcomes without printing prompt,
provider/tool/source/workload bodies or credentials. A captured model-dispatch test uses secret
sentinels to enforce that boundary. An enabled/disabled subscriber parity control compares exact
durable event histories, content identity, and provider-call count. CI also rejects tracing spans
and mechanically detectable fallible, asynchronous, or stateful work inside logging events. The
detailed field/level/coverage ledger is
[`OBSERVABILITY.md`](OBSERVABILITY.md).

Next observability work lands with its owning product slice: candidate/admission lifecycle events,
generic durable Oracle coordination, external API request propagation, metrics export, dashboards,
and alerts. None may make logs or metrics verdict authority.

## Cross-cutting gates

Every phase must keep strong domain IDs, strict versioned JSON, append-only causal facts, secret-free
records, configurable budgets/timeouts/limits, SQLite behind storage ports, and MIT-compatible
dependencies. Ordinary tests must remain offline. Real-host and cross-build gates remain separate,
explicit evidence-producing checks.
