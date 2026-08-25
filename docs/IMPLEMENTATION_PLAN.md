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

## Phase F — one-command open-source worker join

Status: F1 join/bootstrap composition, F2a/F2b typed resumable assignment-material replication, and
F2c controlled local-process activation/create-only materialization implemented. Service-unit
output, hardened hostile-code sandboxing, and real x86-64/AArch64 execution gates remain.

Implement `cairn-worker join` as a composition of enrollment, built-in probe, validated local
profile creation, control-endpoint configuration, fixed state-directory layout, and optional
service-unit output. The controller assigns `WorkerId` and pool; the worker reports resources. Keep
the lower-level `enroll`, probe, validate, and run commands available for automation and debugging.

Acceptance gate:

- a new machine needs one short-lived bundle and one command, with no copied private key;
- generated configuration contains no model/provider or migration-business assumptions;
- architecture is discovered on the target host and checked against optional operator expectations;
- rerun is safe, no differing file is overwritten, and diagnostics identify the exact recovery
  action;
- clean x86-64 and AArch64 hosts join, reconnect, receive a generic assignment, and survive restart.

F1 makes the V1 enrollment bundle self-contained by embedding the independently routable normal
control endpoint and its pinned CA/name. `cairn-worker join <bundle> <state-dir>` creates a fixed
identity/scratch/journal/config tree, hashes the running worker binary, runs the built-in host
probe, and persists a strict V1 worker configuration with explicit execution mode. Its initial
availability remains deliberately
unavailable and draining with `execution.mode=disabled`; enrollment never implies execution
readiness. Re-running join validates and reuses the tree without overwriting operator edits.

F2a established worker-local typed CAS as a prerequisite for admission and start. F2b replaces
inline offer bytes with an immutable manifest of typed identities, exact lengths, and configured
chunk size. While the durable offer remains unacknowledged, the authenticated worker may request
bounded ranges; every chunk is synced to a fixed per-offer staging file, and reconnect resumes from
its exact length. Chunks create no domain facts. Complete assembly must derive the manifest's
`ContentId<T>` values in worker-local CAS before admission; start reopens those objects. The source
CAS is fully verified when creating the manifest, an isolated range-source port makes transfer
linear, and destination verification closes the range-read trust boundary. Aggregate raw-material
limits remain independently optional on controller and worker. Chunk sizes are positive explicit
configuration, and their exact base64-expanded envelope is checked against the separately optional
transport limit before startup. F2c now defines canonical versioned input/environment material,
materializes only directories and regular files into a private create-only per-attempt tree, and
composes an explicitly activated `local-process-v1` supervisor. Activation is a fail-closed
invariant across mode, exact backend claim, and ready availability; join remains disabled. The
adapter clears ambient environment, requires fixed Linux user/network namespace preflight, starts a
new process group, enforces timeout/stream/output bounds, and captures executable/environment
evidence. It is intentionally classified as a controlled-host utility backend rather than
oracle-grade hostile-code filesystem isolation. The next F2 slice must add a hardened container or
equivalent launcher while reusing these material and executor contracts.

### Next slice: F2d hardened OCI container backend

F2d will implement `oci-container-v1` as the first backend allowed to run untrusted CPU-only
candidate/oracle processes. The first runtime adapter will use a configured Docker-compatible CLI,
but product code will depend on a narrow `ContainerRuntime` port rather than Docker command output
or lifecycle vocabulary. Runtime path, state roots, timeouts, limits, and activation remain
operator configuration; isolation capabilities and fixed security arguments belong to the backend
template and cannot be weakened field-by-field in `worker.json`.

The slice is divided into the following implementation steps:

1. **Threat model and typed runtime contract.** Freeze which host resources are invisible to the
   subject and introduce strong image digest, deterministic container name, runtime container ID,
   container phase, mount role, and sandbox-policy types. Add a strict backend-specific OCI
   environment format that pins an immutable image digest rather than a mutable tag. Preserve the
   existing `JobContract`, `AttemptId`, material CAS, executor authority, and receipt lineage.
2. **Fixed CPU-only isolation plan.** Generate argv without a shell for a read-only root filesystem,
   non-root subject, `--network none`, dropped capabilities, `no-new-privileges`, independent PID,
   mount, IPC, user, and network namespaces, bounded PIDs/CPU/memory, and size-bounded writable
   work/output/tmpfs mounts. Input material is mounted read-only. Worker identity, credentials,
   journal, CAS, runtime socket, and host paths are never mounted. V1 rejects device requests and
   all GPU/NPU exposure rather than silently broadening privilege.
3. **Recoverable container supervisor.** Derive one deterministic container identity from the exact
   `AttemptId` and bind job/contract/input/environment identities as immutable labels. Reconcile
   `Absent → Created → Running → Exited` through inspect/create/start/wait operations. Restart may
   reattach to the exact labeled container or collect an exited result, but must never start a
   second subject. A same-name container with conflicting labels fails closed and is not deleted or
   reused.
4. **Bounded capture and trusted evidence.** Drain stdout/stderr independently under contract bounds,
   enforce timeout/output exhaustion by stopping the same identified container, ingest only
   declared regular output files, and record resolved local image ID, runtime identity, fixed policy
   version, container ID, timing, and exit state outside candidate-writable mounts. Cleanup occurs
   only after the terminal worker result is durable; cleanup failure retains evidence and cannot
   turn into re-execution.
5. **Activation and open-source operations.** Extend V1 execution configuration with explicit
   `oci_container` activation and derive the exact `oci-container-v1` worker claim from it. Startup
   preflight verifies CLI/runtime reachability, immutable-image resolution, required isolation
   features, disjoint absolute state roots, and configured optional limits. Join remains disabled.
   Document rootful/rootless prerequisites, diagnostics, state recovery, and an operator smoke
   command without embedding deployment-specific paths.

F2d acceptance requires:

- an escape fixture cannot read worker credentials, SQLite/CAS, runtime socket, or unrelated host
  files, cannot write the input mount/root filesystem, and cannot obtain network connectivity;
- timeout, fork/PID pressure, memory exhaustion, stdout/stderr exhaustion, missing/oversized output,
  nonzero exit, and successful output produce distinct bounded terminal evidence;
- runtime absence or failed preflight is `NotStarted`; uncertainty after create/start is
  `Ambiguous` until exact-container reconciliation proves a terminal state;
- worker/control reconnect and worker/runtime restart recover the same container without a second
  start, including a result completed while the WebSocket is disconnected;
- conflicting names/labels, mutable image tags, extra mounts/capabilities/devices, symlink outputs,
  and policy downgrade attempts fail closed;
- offline fake-runtime contract tests pass in ordinary CI, while opt-in Docker-compatible real-host
  gates pass on both x86-64 and AArch64 and publish inspectable evidence;
- the security documentation continues to classify `local-process-v1` as controlled-host only and
  does not claim that OCI alone protects against a hostile kernel or runtime.

Accelerator/NPU/GPU device containers are intentionally a later F2e slice. They will add explicit
device leases, exact device-node exposure, runtime/driver observations, and post-run quarantine on
top of F2d rather than weakening the CPU container policy.

Worker-control protocol, controller-outbox facts, and worker-journal facts all use schema V1. During
pre-release development, an incompatible format change replaces the V1 definition and development
state is rebuilt; runtime readers do not contain conversion branches.

## Cross-cutting gates

Every phase must keep strong domain IDs, strict versioned JSON, append-only causal facts, secret-free
records, configurable budgets/timeouts/limits, SQLite behind storage ports, and MIT-compatible
dependencies. Ordinary tests must remain offline. Real-host and cross-build gates remain separate,
explicit evidence-producing checks.
