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
catalog now independently resolves alias, wire model, deployment, protocol, capability profile,
generation settings, transport bounds, and external credential reference into a typed secret-free
snapshot. The initial example chooses `deepseek-v4-pro` over an Anthropic-compatible deployment and
provides a Chat Completions alternative. The catalog also models OpenAI Responses as an independent
protocol family for deployments that actually support the selected model; codec selection never
uses provider-name branches. Protocol codecs and live HTTP are the next implementation slice. Later
configuration changes affect new episodes, not historical meaning. The remaining architecture in
the normative documents is still target design. The old repositories are evidence and compatibility
references, not source trees to copy mechanically.

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

## Project principle

> Search for an implementation. Search for a way to falsify it. Record enough evidence to walk the
> entire route again.

## License

Cairn is licensed under the [MIT License](LICENSE).
