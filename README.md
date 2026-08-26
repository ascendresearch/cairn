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

## Current implementation

Cairn is pre-release and all persisted and wire formats remain V1. Incompatible development changes
replace V1 directly; there are no compatibility readers or data migrations.

Implemented foundations include strong domain identities, canonical JSON, append-only SQLite event
stores, filesystem content-addressed storage, durable model/tool/episode lifecycles, provider-native
conversation replay, runtime model templates, managed worker enrollment and credential lifecycle,
host resource probing, deterministic scheduling and reservations, resumable assignment-material
transfer, the first real worker execution backend, and the first domain-neutral oracle-admission
policy and numerical-allowance contracts.

F2 now uses one concrete `docker-v1` adapter. A worker commits the start fact before execution,
reconciles one deterministic container across process restart, captures bounded stdout/stderr and
declared artifacts, durably publishes the terminal observation, and only then cleans up. Resource
bounds are independently configurable or disableable. The deployment assumption is operator-owned
private infrastructure: users are responsible for submitted code and images; Cairn does not attempt
malware detection or hostile multi-tenant isolation.

A real Hello World gate is available:

```bash
scripts/docker-hello-smoke.sh sha256:<64-hex-local-image-id>
```

It executes content-addressed input in Docker and proves that replaying the same exited attempt
returns a byte-identical capture. See [`docs/WORKER_EXECUTION.md`](docs/WORKER_EXECUTION.md).

M2 has begun in `cairn-verification`. Its current V1 foundation keeps admission-policy counts,
required construction/fault classes, execution scope, and budget-exhaustion behavior in immutable
configuration. Numerical allowance provenance and assurance are independent typed facts;
held-out evidence requires identity-disjoint corpora, and asserted or external-prior-only values
cannot be promoted by assurance metadata. The proposal side now has immutable caller-domain,
refinement, corpus, authorship, construction-claim, correct/wrong-variant, and oracle-proposal
manifests with separate typed identity domains. Executed oracle admission, mandatory base-case
execution, the historical reduction control, candidate judgment, and the first unified migration
are not implemented yet. `cairn-migration` now supplies the first strongly typed operator-domain
body plus trusted quantitative, dtype-pattern, and pointer/error-surface derivation. Floating
special-value and memory conditions retain explicit supported/invalid/excluded/unknown
dispositions. Historical target/oracle failure records and exact-domain coverage obligations now
retain typed provenance, scope, stage, and detection requirements. Populating and executing the
historical fixtures remain unfinished. Supported and explicitly-invalid dtype recipes now
materialize into bounded deterministic little-endian bytes with typed element/byte quantities and
content-bound source/byte manifests. Trusted quantitative boundary cases can now rederive their
domain membership, resolve shapes, encode scalar arguments, describe output allocations, and build
an ABI-ordered canonical `InputBundleV1`; bundle/manifest/source identities and exact files are
cross-validated. Supported and explicitly-invalid dtype obligations now share one composition path
over a separately proven successful quantitative baseline, vary exactly one input buffer, and bind
that buffer's exact materialization identity. Supported recipes expect success; invalid recipes
inherit only their declared behavior. Executable memory-surface obligations now assemble over a
trusted successful baseline and emit a distinct typed manifest for one null, misaligned,
short-capacity, exact-alias, or partial-overlap layout. Unknown and excluded conditions remain
non-executable, and actual unsafe address realization stays inside the pending isolated call
adapter. The first adapter-process slice now binds any assembled case to bounded executable bytes,
a typed manifest identity, and a strict one-process/one-invocation request inside a canonical input
bundle, with a fixed non-shell command and sandbox roots. The strict result side now binds
request/invocation identity, pre-invocation rejection versus actual void/status return, and exact
successful output bytes. Invalid-input cases never promote unspecified output buffers into evidence.
Job composition, real CUDA/Ascend C adapters, and complete corpus orchestration remain unfinished.

The remaining architecture in the normative documents is target design. Old Cairn and Alloyport are
evidence references, not source trees to copy mechanically.

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
- [`docs/RESOURCE_PROBING.md`](docs/RESOURCE_PROBING.md) — startup resource facts, operator
  expectations, dynamic observation authority, and quantitative reservation accounting.
- [`docs/WORKER_EXECUTION.md`](docs/WORKER_EXECUTION.md) — Docker activation, recovery, and the
  real Hello World gate.
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
