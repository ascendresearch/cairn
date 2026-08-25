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
- the then-transitional static enrollment cannot collide with managed credential IDs and is later
  removed from runtime authority by Phase E1's explicit migration gate.

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
- frozen contracts and profiles use new V3 identity domains so old bytes cannot gain invented
  quantitative meaning;
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

## Phase E — registry and operator lifecycle closure (E1 implemented 2026-08-25)

Finish migration away from controller static certificate lists. Add `import-static` with collision
and ownership checks, list/show/audit commands, worker re-enable, credential inspection, and
separate pool reassignment. Keep worker, credential, enrollment, and pool histories distinct. Add a
versioned migration gate for pre-release server configuration and worker-registration facts.

E1 implements the static-authority migration boundary. `registry import-static` atomically freezes
the canonical credential/fingerprint/worker/pool batch under an explicit `CommandId`; exact retries
recover the original event while changed input or any ownership collision fails closed. Controller
schema V3 requires `enrollment: []`, and authentication plus scheduling now consume only the
persistent registry. E2 retains list/show/audit, worker re-enable, credential inspection, separate
pool reassignment, and explicit-command upgrades for the earlier lifecycle commands.

Acceptance gate:

- a static deployment can be imported without changing `WorkerId` or losing provenance;
- ordinary startup no longer needs a copied certificate list;
- every lifecycle command is idempotent under an explicit command identity and produces an
  auditable fact;
- revoke, disable/re-enable, pool change, and import survive restart and reject contradictory
  histories.

## Phase F — one-command open-source worker join

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

## Cross-cutting gates

Every phase must keep strong domain IDs, strict versioned JSON, append-only causal facts, secret-free
records, configurable budgets/timeouts/limits, SQLite behind storage ports, and MIT-compatible
dependencies. Ordinary tests must remain offline. Real-host and cross-build gates remain separate,
explicit evidence-producing checks.
