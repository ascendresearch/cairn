# Cairn

Cairn is an open-source, evidence-driven system for migrating previously unseen CUDA software to
hardware-affine Ascend C implementations.

The first supported hardware target is **Ascend 950PR (3510)**. SoC, CANN/toolchain and execution
environment remain explicit task inputs so evidence from another target cannot be silently reused.

The goal is not to translate syntax or merely generate plausible code. Cairn is intended to deliver
a reviewable migration package containing target-specific Ascend C kernels, host tiling and
integration code, executable validation, performance evidence, known limitations, and exact replay
lineage.

## Why Cairn

CUDA and Ascend C expose materially different execution models. A useful migration must recover the
required computation, decide which CUDA behavior should actually be preserved, construct a judge for
future Ascend implementations, search hardware-specific tiling and pipeline strategies, execute on
real toolchains and devices, and explain the remaining risk.

Cairn therefore treats the runtime model as a per-task reasoning actor, not as an authority:

- models inspect unfamiliar source, propose intent, experiments, validation mechanisms and code;
- the Controller freezes inputs, capabilities, revisions and workflow transitions;
- ordinary managed Workers perform CUDA, CPU/reference and Ascend build/run/profile effects;
- mechanical Admission functions recompute what may be trusted;
- every expensive or external result is bound to exact artifacts and replayable receipts.

The repository coding agent builds this generic system. It must not interpret a known fixture and
encode that answer into production prompts, APIs, policies or task construction.

## Accepted task shapes

PyTorch is optional. A task may contain only CUDA source, a host launcher, build files and a caller
declaration. Cairn can also use additional authorized evidence when present:

- C/C++ tests or executables;
- CPU or Python references;
- PyTorch custom operators, OpInfo or framework tests;
- TensorFlow, JAX, Triton or other integration code;
- specifications, papers or model-graph context;
- production shape, dtype and workload traces.

The input CUDA implementation is evidence, not a guaranteed specification. Observed CUDA bugs,
races, undefined behavior or accidental numerical behavior do not automatically become migration
requirements.

## Product direction

Cairn is Ascend-C-first, with Ascend 950PR (3510) as the initial product target. Existing Ascend
libraries or higher-level implementations may be used as
references, baselines, seeds or explicit escape hatches, but the long-term differentiator is the
ability to generate new implementations for a specific Ascend SoC and CANN/toolchain—even when
framework and template-library support is immature.

The system is organized around four connected planes:

1. **Semantic Contract** — recover and admit what must be migrated;
2. **Platform Facts** — measure what the exact target hardware and toolchain support;
3. **Implementation Search** — generate and evaluate an Ascend C candidate family;
4. **Assurance and Delivery** — validate, benchmark, replay and package the result.

Generated output may include multiple specialized kernels and a verified tiling/dispatch policy.
Hardware affinity is not assumed to have one globally optimal implementation.

## Workflow

```text
CUDA task + caller + target
        │
        ▼
Migration reasoning ── source/reference/target experiments
        │                          │
        │ material semantic fork  │
        ├── focused SIR ───────────┤
        │                          │
        ▼                          ▼
Intent contract + evolving Evidence/Assurance Graph
        │                          │
        ├── exploratory Ascend C candidate-family search
        │             └── build / NPU run / profiling feedback
        │
        ▼
sealed-policy coverage challenge + qualified Validation Bundle
        │
        ▼
Qualification Epoch ── honest / correct-variant / mutant / hidden controls
        │
        ▼
Oracle Admission → Candidate Promotion/Admission
        │
        ▼
Reviewable Migration Package
```

Source understanding occurs in every migration, but Cairn does not run a mini-SIR classifier before
deciding whether to run SIR. A focused SIR protocol is materialized only when actual migration
reasoning exposes a semantic fork that changes the candidate or its judge. An exploratory candidate
may exist before the final Oracle is accepted, but it has no release authority.

Intent, assurance, Oracle and Candidate roles use real model/tool Agent Loops. The outer traversal
over graph nodes, revisions, experiments, controls and candidates is mechanical Controller
orchestration, not nested Agent Loops or separate proposal processes.

The normal customer and execution path is:

```text
cairn-cli → cairn-server → migration app API → CudaMigrationWorkflow → managed Workers
```

## Why the workflow is structured

Cairn does not depend on an Agent remembering every obligation in one long response. Structured
claims, focused items, independent reviews, feedback and revision loops help models concentrate and
make omissions visible.

Structure is not proof. A filled schema, repeated self-review or model consensus cannot replace a
compiler result, execution receipt, independent reference, counterexample, mutation control or
mechanical recomputation. Decomposition is intended to be risk-adaptive: difficult numerical,
concurrent, stateful or reference-free tasks receive deeper review and experimentation than simple,
well-specified mechanisms.

## Current status

Cairn is pre-release. All internal formats remain current V1 definitions; incompatible development
changes update V1 directly without compatibility readers, converters or migration paths.

The repository already contains substantial foundations for:

- strong domain identities and validated serialization;
- durable event, content and Agent episode records;
- model/tool execution and continuation recovery;
- managed Worker enrollment, scheduling, execution and receipts;
- the normal CLI/server/migration-workflow submission path;
- SIR and structured Oracle role Agent Loops;
- claim-scoped Oracle exploration and mechanical controls;
- exact diagnostic authority and safe operational logging.

The first complete CUDA→Ascend C migration package has not yet been established. Current development
is measuring whether up-front structure or adaptive evidence-driven co-design reaches the same
release gates more reliably and economically.

The first non-`vectorAdd` dogfood audit also found that the bundled Oracle Worker runner validated
plan structure rather than executing candidate-facing mechanisms. That path now fails closed; real
Oracle acceptance remains blocked until typed Worker experiments and executable qualification are
connected. See
[`DEV-037-IMPLEMENTATION.md`](docs/dev/records/DEV-037-IMPLEMENTATION.md).

## Preserved pilots and next ablation

The preserved A/B/C runs are implementation pilots, not causal comparisons:

| Mode | Reasoning decomposition | New Worker evidence |
| --- | --- | --- |
| A `MinimalDecomposition` | one SIR episode and one whole-portfolio Oracle episode | no |
| B `StructuredReview` | dimension/item discovery, development, review, revision and coherence | no |
| C `EvidenceAugmentedStructuredReview` | the same structured workflow | yes, typed CUDA/CPU/reference/Ascend requests |

They showed that whole-portfolio reasoning can be broad, independent Review can find real defects,
and Worker evidence can change a revision—but also that decomposition can explode and the mere
existence of a Worker does not provide the required CUDA or 950PR capability.

The proposed next comparison is:

- **D-Upfront**: blind Oracle scope, sealed-policy challenge and full structured Review before the
  candidate workflow;
- **E-Adaptive**: intent, assurance and an authority-restricted exploratory candidate co-evolve,
  followed by a late sealed-policy challenge and full D fallback when required;
- **E-Full-D-Fallback** and an organic-only diagnostic arm to isolate the value of the safety net.

Candidate revisions do not win because they are newer. Promotion binds one admitted Intent,
Qualification Oracle, 950PR target, Candidate revision and promotion policy into an immutable epoch.
If the Oracle changes, the parent and new candidate are replayed symmetrically. Performance and
precision improvements are predeclared, correctness-gated and evaluated against independent
controls with bounded hidden-query exposure.

The evaluation records obligation coverage, incorrect intent promotion, false acceptance/rejection,
required capability closure, candidate promotion validity, Oracle symmetric replay, hidden-control
exposure, reviewer/evidence value, tokens, elapsed time and Worker/device cost.

The corpus must include both framework-free CUDA tasks and tasks with optional framework/reference
material. PyTorch is one useful adapter, not an experiment prerequisite.

## Design baseline

Start with the two current authoritative documents:

- [`docs/design/CAIRN_CURRENT_PRODUCT_DESIGN.md`](docs/design/CAIRN_CURRENT_PRODUCT_DESIGN.md) — the
  product mission, four-plane architecture, coupled workflows, delivery contract and ablation plan;
- [`docs/design/SIR_ORACLE_CURRENT_DESIGN.md`](docs/design/SIR_ORACLE_CURRENT_DESIGN.md) — detailed
  focused SIR, evidence graph, evolving/qualification Oracle, Worker/control and Candidate promotion
  boundaries.

The broader candidate E and its completeness audit are in
[`docs/design/EVIDENCE_DRIVEN_ADAPTIVE_MIGRATION_DESIGN.md`](docs/design/EVIDENCE_DRIVEN_ADAPTIVE_MIGRATION_DESIGN.md).

Earlier documents remain useful historical evidence, but they do not add implicit requirements to
the current design.

Repository rules are defined by [`AGENTS.md`](AGENTS.md).

## Local verification

The focused migration crates can be checked with:

```bash
cargo test -p cairn-migration -p cairn-migration-app --all-targets --no-fail-fast
cargo clippy -p cairn-migration -p cairn-migration-app --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Billable model calls, CUDA execution and Ascend Worker runs are explicit opt-in operations and are
not part of ordinary unit tests.

## Project principle

> Recover the contract. Search the target hardware. Try to falsify the result. Deliver the code and
> enough evidence to reproduce the claim.

## License

Cairn is licensed under the [MIT License](LICENSE).
