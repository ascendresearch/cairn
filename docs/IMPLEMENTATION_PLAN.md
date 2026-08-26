# Integrated implementation plan

- Status: active
- Date: 2026-08-25
- Scope: worker authority, scheduling, resource discovery, registry transition, and open-source onboarding

This plan combines the unfinished parts of resource-driven workers and managed enrollment into one
dependency-ordered program. It is an implementation plan, not a replacement for the normative
requirements and system design. A later phase may start only when the preceding phase's acceptance
gate is durable across restart.

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

## Phase G — executed oracle admission (active)

The next product milestone is M2, not a wider worker runtime. Its first implemented foundation is a
new domain-neutral `cairn-verification` crate with strict V1 admission-policy and numerical-allowance
contracts. Policy owns variant minima, required construction/fault classes, structural independence,
saturation, accepted strengths, execution scope, and budget-exhaustion outcome. Numerical provenance
and assurance are separate; held-out derivation and validation corpora must be identity-disjoint;
asserted and external-prior-only values cannot support a pass; held-out evidence is empirical at
most; and only proven/exhaustive evidence may support an unqualified domain-wide numerical claim.

The remaining G slices, in dependency order, are:

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
   unknown and excluded conditions are rejected. Actual pointer construction, call-adapter
   execution, observations, and complete corpus orchestration are still pending. The isolated
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
   vendor adapters and execution-receipt ingestion remain pending.
   Strict historical record/obligation/coverage contracts now bind provenance, target-oracle scope,
   observed stage, required detector, exact record identity, domain family, and caller-domain
   identity;
3. versioned mutation-grid trials and recomputed proof obligations that ignore stored `passed`
   metadata;
4. execution-port composition for correct and deliberately wrong variants through the complete
   build/execute/observe/compare path;
5. the offline historical reduction control required by `ORACLE_ADMISSION.md` section 18;
6. immutable admitted-oracle and candidate-verdict receipts with complete identity-graph audit.

Phase G is incomplete until that historical control reproduces the old false reject, accepts the
correct tree reduction under measured family spread, retains the known blind spot, rejects asserted
allowance and an empty applicable mutation grid, and emits a complete admitted-oracle graph. Open
questions OQ-004 and OQ-007 remain unresolved; implementation must preserve derivation and
disagreement evidence rather than selecting an independence or automatic disagreement policy.

## Cross-cutting gates

Every phase must keep strong domain IDs, strict versioned JSON, append-only causal facts, secret-free
records, configurable budgets/timeouts/limits, SQLite behind storage ports, and MIT-compatible
dependencies. Ordinary tests must remain offline. Real-host and cross-build gates remain separate,
explicit evidence-producing checks.
