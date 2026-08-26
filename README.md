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

The managed live deployment also has opt-in gates for Alloyport's original CUDA reduction intake
and a no-device Ascend C toolchain fixture. The CUDA gate compiles and executes all nine release
cases on the GB10 twice; the Ascend gate compiles and links `dav-3510` code twice through a separate
device-free build worker. These are authoritative execution receipts for the source reference and
target compiler substrate, not yet a generated target reduction candidate or device verdict.

M2 now includes the first model-authored Oracle Agent control. Separate cache-aware Blue and Red
episodes freeze independent model, budget, private-context, and tool-catalog edges. Blue can search
bounded operator-approved GitHub repositories for exact upstream test bytes and license/provenance
evidence; those results remain proposals and cannot bypass trusted admission. Provider cache usage
is retained as optional metering evidence, and native role prefixes reconstruct byte-identically
across restart. Blue proposals and Red correct/wrong attacks carry exact model authorship into an
immutable revision/feedback graph. The historical reduction gate now traverses that model-authored
boundary before producing its hardware-free admitted oracle.

The current V1 verification foundation keeps admission-policy counts,
the exact trusted generic-mutant set, required construction/fault classes, execution scope, and
budget-exhaustion behavior in immutable configuration. Numerical allowance provenance and assurance
are independent typed facts;
held-out evidence requires identity-disjoint corpora, and asserted or external-prior-only values
cannot be promoted by assurance metadata. The proposal side now has immutable caller-domain,
refinement, corpus, authorship, construction-claim, correct/wrong-variant, and oracle-proposal
manifests with separate typed identity domains. The first immutable admitted-oracle receipt is now
implemented for the hardware-free historical reduction control. That frozen oracle can judge
candidate-role host reduction executions and emit recomputable pass/fail receipts; general
candidate search, production CUDA/Ascend call adapters, target-device judgment, and the first
unified migration are not implemented yet.
`cairn-migration` now supplies the first strongly typed operator-domain
body plus trusted quantitative, dtype-pattern, and pointer/error-surface derivation. Floating
special-value and memory conditions retain explicit supported/invalid/excluded/unknown
dispositions. Historical target/oracle failure records and exact-domain coverage obligations now
retain typed provenance, scope, stage, and detection requirements. The reduction false-reject
fixture is populated and executed; target-specific historical fixtures remain unfinished. Supported
and explicitly-invalid dtype recipes now
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
Prepared adapter processes now compose into the existing vendor-neutral `JobContract` with explicit
environment, resources, disabled network, capture bounds, canonical bytes, and typed contract
identity; migration tiers remain outside worker records. An authoritative generic execution receipt
can now be bound back to the exact prepared job, loaded through typed declared-output identities,
and validated as the adapter result plus ABI observation; a CAS object alone is not execution
authority. A deterministic Rust host fixture now exercises this entire transport path with an
actual adapter process, durable coordinator facts, restart recovery, and exact output-byte identity.
A canonical complete-corpus plan now binds the quantitative, dtype, and memory obligation-set roots,
the exact source/reference/property/admission-variant/candidate subject role and upstream artifact,
one executable, and one independently identified generic job per executable obligation. Unknown and
explicitly excluded obligations remain committed but do not silently become jobs; missing,
duplicate, extra, reordered, cross-domain, or unrecognized-role cases fail closed. Real CUDA/Ascend
C adapters, device
execution of the complete plan, and semantic adjudication remain unfinished. Given authoritative
receipts, strict complete-corpus collection now requires exactly one receipt per planned `JobId`,
revalidates every category-specific adapter result and declared output, and emits a canonical
observation-set identity bound to the exact plan. The set records execution observations, never a
pass/fail verdict. The host adapter is a protocol fixture, not an admitted oracle or a production
unsandboxed executor. For domains explicitly requesting exact semantics, a role-safe comparison
slice now accepts `Reference` versus either `Candidate` or `AdmissionVariant` plans over identical
mandatory obligations, aligns every typed case, and records reference and subject completion values
plus exact ABI-output identities. Match status is recomputed from those facts; the artifact has no
stored `passed` field and does not promote a proposed reference into an admitted oracle or trusted
adjudication.
Trusted mutation evidence now has its own domain-neutral V1 contracts. A policy selects one exact
content-addressed generic-mutant set; a complete grid requires every mutant/case cell and records
policy-sized, scale-free, case-dependent, or explicitly non-injectable trials with separately typed
injection, execution, and comparison evidence. Proof obligations are recomputed from the grid:
policy-sized and scale-free misses are fatal, case-dependent misses remain mandatory blind spots,
an empty applicable grid fails, and comparator-only evidence cannot claim to have exercised the
implementation observation path. The proof artifact has no stored `passed` field. An exact host
composition gate now binds each correct or deliberately wrong admission variant to its implementation
bytes, a fixed no-shell build job, an authoritative generic receipt, the exact produced adapter
executable, a complete admission-variant corpus plan, observations, comparison, and recomputed
`MustAccept`/`MustReject` expectation. Its build fixture is an identity-preserving host fixture rather
than a compiler, and the complete corpus uses deterministic execution captures after separately
exercising the real build and representative adapter processes. The historical reduction adapter
now provides the first actual mutation composition: it binds closed drop-last, unit-offset, and
zero-output mutant kinds to the exact wrong implementations, fault evidence, builds, and real runs,
then derives a complete 3-by-2 grid from exact per-case ULP comparisons. Product candidate compiler
composition and full device execution through vendor adapters remain unfinished. The first
hardware-free historical reduction control now loads the domain/reference/corpus through ordinary
proposal artifacts, builds and executes two distinct correct and three distinct wrong compiled host
variants through authoritative generic receipts, and records exact finite-f32 bits plus recomputed
ULP distances. It reproduces the old zero-ULP single-sample false reject, derives a one-ULP
measured-family allowance that accepts the correct balanced tree, makes all three wrong variants red,
and retains a real case-dependent drop-last blind spot on a trailing-zero case. Asserted allowance,
an empty applicable mutation grid, mutant/algorithm relabeling, changed ULP or receipt/output
identities, a stored `passed` field, and non-V1 input fail closed. The
validated control now emits a strict admitted-domain manifest, complete admission receipt, and
immutable `AdmittedOracle`. These artifacts freeze the proposal, policy, empirical reference
strength, corpus, allowance, environments, variant and mutation evidence, historical coverage,
case-dependent blind spot, assumptions, target/device claims left unverified, and explicit
revalidation triggers. Missing saturation rounds or any changed receipt/oracle edge fail closed.
Candidate execution has a distinct role and authoritative run receipt. The reduction judgment path
recomputes every reference/candidate ULP distance against the frozen one-ULP allowance, emits all
failed cases, carries oracle blind spots/assumptions/unverified target claims forward, and derives
`Pass` or `Fail` without a stored boolean. A balanced-tree candidate passes, a zero-output candidate
fails, and an admission-variant run cannot be relabeled as candidate evidence.

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
