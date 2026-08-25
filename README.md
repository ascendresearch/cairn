# Cairn

Cairn is an open-source, evidence-first agentic engineering system for searching, executing,
verifying, and replaying heterogeneous software migrations.

Its first product slice migrates CUDA operators to Ascend C. Cairn does not treat generated code as
the product by itself: the product is an implementation, a verdict on that implementation, and an
auditable evidence chain showing what was tried, what was measured, what remains unverified, and why
the verdict is entitled to say what it says.

Cairn is a ground-up Rust rewrite of the earlier `cairn` agent harness and `alloyport` migration
system. They are no longer separate products. Agent execution, remote execution, oracle admission,
candidate search, verification, records, replay, and counterfactual experiments belong to one system
with internal architectural boundaries.

The repository is in early implementation. The Rust workspace, strict canonical JSON V1 boundary,
typed SHA-256/UUIDv7 identities, append-only event-store port, streaming content-store port, SQLite
metadata adapters, filesystem blob CAS, model-input projection/audit, and recorded/scripted model
transports are implemented. Model dispatch now has a durable prepared/started/terminal lifecycle,
one-shot typed authority, exact response archiving, explicit ambiguous-effect recovery, and causal
history validation. Tool operations use the same durable effect discipline with canonical argument
and result artifacts, recorded/scripted gateways, and effect-class-derived retry or reconciliation
semantics. Version-pinned model adapters now archive provider-neutral semantic turns and atomically
publish derived, non-authoritative tool-call proposals that can be rebuilt after restart. The
neutral agent-step projection exposes only the next safe durable action, including explicit
in-doubt, decode, yield, and awaiting-operation boundaries. Trusted tool registrations can now bind
ordered proposals atomically to unique logical operations without granting execution authority.
Concrete tool invocations carry their own `AttemptId`; completed and definitively rejected outcomes
flow back as verified `pending_results` in proposal order, while retry and reconciliation cases stay
typed blockers. Retry now requires a new durable authority and rejects reused attempt identities.
Reconcile-required effects can only resume after citing verified, content-addressed evidence that
either proves the effect did not occur or confirms the original attempt's result. The remaining
agent loop now has a durable episode aggregate that pins task and role identities, grants one-shot
step authority, verifies cross-step `pending_results` lineage, survives lost open/advance
acknowledgements, and terminates at yielded, step-limit, deadline, or tool-operation-budget safe
points. Logical tool-operation budget is reserved by an episode admission fact before step binding;
recovery verifies the exact operation IDs and trusted registrations across both streams, and no tool
authority is produced when admission would exceed the limit. Model transports can attach validated
provider input/output-token receipts to the same durable response fact as the archived bytes. An
optional episode token threshold accumulates only those receipts: reaching it blocks the next model
step, while a missing receipt fails closed instead of being treated as zero. The remaining
episode budget dimensions—step count, logical tool operations, observed provider tokens, deadline,
and named external meters—are independently configurable in serialized `EpisodeBudget`: a typed
value enables a dimension and `null` or an omitted field disables it. External actions use their
own `MeteredActionId` and a durable reserve/start/receipt saga. Reservation commits before execution;
recovery distinguishes denied, ready, in-doubt, and receipted actions and re-evaluates ledger facts
against the budget frozen in `EpisodeOpened`. This is the capability boundary for future live
integrations; it is not yet wired into a concrete external-service adapter. A strict runtime-model
catalog now combines two separate inputs. Repository-owned files under `model-templates/` describe
wire model identity and per-protocol capabilities/defaults; user configuration selects an enabled
template, protocol, endpoint, credential reference, data boundary, transport limits, and optional
bounded overrides. The initial example enables `deepseek-v4-pro` over OpenAI Responses, while its
template also describes Chat Completions and Anthropic Messages. A private endpoint changes only
deployment configuration, and codec selection never uses provider-name branches. Resolution freezes
the exact template identity and a typed secret-free snapshot. Protocol-native continuation now
preserves Responses items, Chat assistant messages, Anthropic blocks, thinking state, and tool
correlations in a sensitive typed CAS domain. Native request state is independently typed and
recoverable; one response parse atomically publishes native, semantic, and tool-proposal facts. The
durable `AgentStep`/`AgentEpisode` path has completed a two-step tool loop with trusted execution,
crash-boundary recovery, and a byte-identical reconstructed second request. A bounded HTTPS
transport resolves credentials only at dispatch, disables redirects, archives raw responses,
extracts provider usage receipts, and retains ambiguous-effect semantics. The opt-in DeepSeek
Responses check has also completed a real tool-call continuation across a SQLite/CAS close-reopen
boundary without printing thinking or answer content. Later configuration changes affect new
episodes, not historical meaning.

The first execution-control slice is now implemented in `cairn-execution`: opaque versioned job
contracts have typed input/environment/backend/argv/resource/policy/output dimensions, configurable
capture bounds, and content identities that change when any immutable dimension changes. One-shot
attempt authority commits before an executor can run. SQLite/CAS recovery reconstructs authorized
work, treats a started attempt with no terminal fact as in-doubt, allows retry only under a fresh
attempt identity after a proven-not-started or completed terminal state, and revalidates complete
receipts against their frozen contract. Recorded/scripted executors exercise the seam without local
process authority. Candidate-writable stdout, stderr, and declared outputs occupy separate
untrusted content domains from canonical trusted supervisor evidence.

The controller-side worker slice now adds authenticated stable `WorkerId` ownership, unique
`WorkerIncarnationId` processes, content-addressed static profiles, separate dynamic availability
heartbeats, deterministic capability matching, and configurable session/assignment timeouts.
Duplicate live incarnations are rejected; replacement is accepted only after explicit disconnect or
a durably checkable predecessor-expiry boundary. Every execution `AttemptId` owns one assignment
stream, preventing parallel leases after restart. A worker must durably accept a lease and still be
the current live incarnation before the controller commits `AttemptStarted`. Pre-start expiry may be
re-placed under fresh `AssignmentId`/`LeaseId` values; post-start expiry yields reconciliation, never
a retry signal. The control plane is now runnable: workers establish outbound mutually authenticated
TLS WebSockets, exchange strict binary canonical-JSON hello/welcome/heartbeat/control messages, and
bind the verified leaf-certificate fingerprint to an enrolled strong `WorkerId`. Controller and
worker use independent SQLite authorities for durable delivery and recovery; configurable wire,
timeout, heartbeat, polling, reconnect, and diagnostic controls can each be explicitly disabled
where optional. A two-worker integration test proves that distinct certificates and journals become
two recoverable live sessions, while post-commit heartbeat acknowledgements keep independently
idle-bounded connections stable. Worker profile V2 now reports the built-in-observed native
architecture/OS/target environment separately from operator-declared backends and capabilities,
with provenance retained for every claim. Operator `expected_platform` values fail closed instead
of overriding detection. Controller-authorized worker pools and domain-neutral placement requests
let future `cairn-migration` stages ask for resources without assigning business roles to workers.
Managed enrollment now lets the controller emit one expiring bundle while each worker generates and
retains its own private key. Exact-CSR replay closes a lost-response window, and a fresh controller
recovers certificate-to-`CredentialId`/`WorkerId`/pool authority from the append-only registry.
Stable worker principal, rotatable credential, and process incarnation are now distinct in durable
registration facts. Separate append-only actions revoke an unused enrollment, revoke one managed
credential, or disable a logical worker; inactive authority is rejected before registration and is
rechecked for live sessions. Rotation authorities now bind one exact active predecessor, preserve
stable worker/pool identity, issue a fresh worker-local key, and freeze a configurable optional
overlap. Per-rotation immutable staging plus an atomic identity manifest closes response and commit
loss windows. A running worker detects cutover, reconnects under a fresh incarnation, and can
restore a predecessor when a failed successor is revoked before retirement. See
[`docs/ENROLLMENT.md`](docs/ENROLLMENT.md) for the operator flow. Reproducible release tooling
cross-links controller and worker
bundles for x86-64 and AArch64 against a pinned GLIBC baseline and verifies their ELF contracts
before deployment. The current worker executor deliberately returns `NotStarted`; real
local/container backends, global scheduling, richer resource probing, static-registry import, and
real-host job execution remain subsequent slices. The active
dependency-ordered roadmap is [`docs/IMPLEMENTATION_PLAN.md`](docs/IMPLEMENTATION_PLAN.md).

The remaining architecture in the normative documents is still target design. The old repositories
are evidence and compatibility references, not source trees to copy mechanically.

## Authoritative documents

Start with [`docs/README.md`](docs/README.md). The normative baseline is:

- [`docs/SYSTEM_REQUIREMENTS.md`](docs/SYSTEM_REQUIREMENTS.md) — what Cairn must do and how each
  requirement can be accepted.
- [`docs/SYSTEM_DESIGN.md`](docs/SYSTEM_DESIGN.md) — the target architecture, data model, workflows,
  trust boundaries, and deployment shape.
- [`docs/ORACLE_ADMISSION.md`](docs/ORACLE_ADMISSION.md) — how an oracle earns the right to judge a
  candidate.
- [`docs/RECORD_REPLAY.md`](docs/RECORD_REPLAY.md) — the durable event record, content identities,
  reconstruction, replay, and counterfactual execution.
- [`docs/DECISIONS.md`](docs/DECISIONS.md) — resolved architecture choices and their boundaries.
- [`docs/OPEN_QUESTIONS.md`](docs/OPEN_QUESTIONS.md) — decisions deliberately left unresolved.
- [`docs/RELEASE.md`](docs/RELEASE.md) — pinned cross-link toolchain, reproducible bundles, and the
  real-host deployment gate.
- [`docs/IMPLEMENTATION_PLAN.md`](docs/IMPLEMENTATION_PLAN.md) — the integrated authority,
  scheduling, probing, registry, and onboarding delivery plan.

## Project principle

> Search for an implementation. Search for a way to falsify it. Record enough evidence to walk the
> entire route again.

## Opt-in live conformance

The DeepSeek validation is intentionally not part of ordinary tests because it performs billable
network calls. Put a raw API key (one line, no quotes) at
`.cairn/secrets/deepseek-api-key`, restrict it to the current user, review
[`config/live-conformance.example.json`](config/live-conformance.example.json), then run:

```bash
chmod 600 .cairn/secrets/deepseek-api-key
cargo run -p cairn-agent --example deepseek_responses_live -- \
  config/live-conformance.example.json
```

The tool requires one `echo_fixture` call, resumes from its deterministic result after restart, and
prints only typed identities, token usage, and boolean closure checks. It does not print the key,
model thinking, tool arguments, or answer content.

## License

Cairn is licensed under the [MIT License](LICENSE).
