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
transports are implemented; the remaining architecture in the normative documents is still target
design. The old repositories are evidence and compatibility references, not source trees to copy
mechanically.

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
