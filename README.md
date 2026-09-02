# Cairn

Cairn is an open-source system for migrating previously unseen CUDA operators and kernel families
to hardware-affine Ascend C implementations.

The first target is **Ascend 950PR (3510)**. Target SoC, CANN/toolchain, runtime environment and
workload are explicit task inputs; evidence from another target never transfers implicitly.

Cairn is not a syntax translator. It is intended to deliver a reviewable migration package:

- Ascend C kernels, host tiling, dispatch, build and integration code;
- an explicit contract describing which source behavior must be preserved;
- executable correctness, numerical, integration and safety validation;
- target-side performance measurements and supported workload scope;
- limitations, unknowns and exact replay lineage.

## Why Cairn

CUDA and Ascend C expose different execution and memory models. A useful migration must recover the
required computation, distinguish desired semantics from source bugs or accidental behavior, build
a judge for future implementations, and search target-specific tiling and pipeline strategies on
real toolchains and hardware.

The runtime model is therefore a per-task reasoning and generation actor, not an authority:

- model-backed strategies inspect unfamiliar code and propose intent, experiments and candidates;
- the Controller freezes inputs, revisions, budgets, capabilities and workflow transitions;
- managed Workers compile, run, sanitize and profile exact artifacts;
- mechanical Admission gates recompute what may be trusted from frozen policy and receipts.

The repository coding agent builds this generic system. It must not interpret a known fixture and
encode the answer into production prompts, APIs, policies or task construction.

## Product workflow

```text
CUDA task + caller contract + exact target
                    │
                    ▼
Controller-owned Candidate Search Loop
  ├── Exploration Actor episodes
  ├── optional focused semantic investigation
  ├── CUDA / reference / Ascend Worker experiments
  ├── evolving intent and assurance evidence
  └── correctness-first candidate-family search
                    │
                    ▼
freeze search generation and qualification policy
                    │
                    ▼
independent Qualification Epoch
  ├── Oracle adequacy controls
  ├── candidate correctness / numerical / safety controls
  ├── exact 950PR execution and performance
  └── parent/current symmetric comparison
                    │
                    ▼
reviewable Migration Package or an honest partial/blocked outcome
```

Qualification is outside development search. A disclosed hidden case becomes a public regression
and cannot continue to count as independent evidence. A new intent, Oracle, target, candidate or
promotion policy creates a new qualification epoch.

## Current status

Cairn is pre-release. The durable Agent runtime, typed records, managed execution foundation,
CLI/server entry path, intent and Oracle workflow pieces, and several mechanical boundaries exist.
The first complete CUDA-to-Ascend C migration package has **not** been produced. There is no current
claim of native build success, NPU correctness, target performance or end-to-end product success.

See [current implementation status](docs/IMPLEMENTATION.md) for the exact evidence boundary.

## Documentation authority

The repository intentionally has a small current documentation set. Git history carries superseded
designs, development records and experiment artifacts.

| Document | Purpose | Authority |
| --- | --- | --- |
| [AGENTS.md](AGENTS.md) | repository rules and non-negotiable builder boundaries | highest |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | current product and system design | normative |
| [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md) | implemented facts, gaps and next milestone | factual |
| [docs/EVALUATION.md](docs/EVALUATION.md) | how product and architecture claims are tested | normative for evaluation |
| `docs/DECISIONS.md` | current-V1 fixture's identity-bound historical path | machine anchor, non-authoritative |
| `README.md` | public orientation and navigation | summary only |

If code and architecture differ, the difference is an implementation gap or a design change to be
made explicitly. It is not a reason to add a compatibility path. Until a public compatibility
baseline is declared, all internal formats remain the single current V1 definition.

## Local verification

The full repository gate is:

```bash
scripts/ci.sh
```

Focused migration checks are:

```bash
cargo test -p cairn-migration -p cairn-migration-app --all-targets --no-fail-fast
cargo clippy -p cairn-migration -p cairn-migration-app --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Billable model calls and CUDA/Ascend hardware runs are explicit opt-in operations and are not part
of ordinary unit tests.

## Project principle

> Recover the contract. Search the target hardware. Try to falsify the result. Deliver the code and
> enough evidence to reproduce the claim.

## License

Cairn is licensed under the [MIT License](LICENSE).
