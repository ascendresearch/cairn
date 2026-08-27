# Open questions and decision backlog

- Status: non-normative backlog
- Date: 2026-08-27

This file holds decisions that would materially affect the normative requirements or design. An open
question is not permission for an implementation to choose silently. Resolve a question by updating
the affected normative documents and recording the control, measurement, or argument that decided
it.

## Priority meanings

- **P0** — blocks the first implementation boundary or would force another development-state reset;
- **P1** — must be answered before the first end-to-end unified migration;
- **P2** — may wait until a second operator or public platform surface creates evidence;
- **P3** — deliberate future work; do not build ahead of a trigger.

## Resolved questions

- **OQ-001** — canonical structured encoding: resolved by
  [`D-001`](DECISIONS.md#d-001--canonical-json-first-behind-a-codec-boundary). V1 uses canonical JSON
  behind a codec boundary.
- **OQ-002** — reference persistence: resolved by
  [`D-002`](DECISIONS.md#d-002--sqlite-first-behind-storage-ports). V1 uses SQLite behind event,
  projection, coordination, and content-store ports.
- **OQ-003** — structured-domain authority: resolved by
  [`D-003`](DECISIONS.md#d-003--hybrid-authority-for-the-structured-domain). The caller supplies a
  minimum structured contract; Cairn proposes refinements and independently challenges every source.
- **OQ-005** — variant sufficiency: resolved by
  [`D-004`](DECISIONS.md#d-004--policy-configured-variant-sufficiency). Counts, class coverage, and
  stopping rules are versioned `AdmissionPolicy` configuration rather than verifier constants.
- **OQ-008** — numerical allowance confidence: resolved by
  [`D-005`](DECISIONS.md#d-005--separate-numerical-provenance-from-assurance). Allowance provenance
  and assurance are separate; `HeldOutValidated` may produce only an explicitly empirical `Pass`.
- **OQ-013** — identity algorithm and agility: resolved by
  [`D-007`](DECISIONS.md#d-007--typed-sha-256-identities-with-a-pre-release-v1-reset-policy). V1
  uses typed, domain-separated SHA-256 identities and UUIDv7 lifecycle IDs; pre-release changes
  rebuild development state and add no runtime alias or migration framework.
- **OQ-018** — product generalization: resolved by
  [`D-024`](DECISIONS.md#d-024--cairn-product-scope-is-cuda--ascend-c). Cairn's product scope is
  CUDA → Ascend C; domain-neutral infrastructure does not broaden that scope.

## OQ-004 — Independence of semantic reference

- Priority: P1
- Affects: `authority_admitted`, correlated failures

Should a high-precision reference be required to be derived without reading the source CUDA
implementation when an independent PyTorch/specification artifact exists?

Strong independence improves the argument. Some operators have no independent definition and would
become unverifiable at reference strength.

Possible policy: record derivation relationship and admit different strengths rather than impose one
binary rule. Needs fixtures for independent, source-derived, and absent references.

## OQ-006 — Metamorphic relation admission

- Priority: P2
- Affects: non-reference oracle strength

How many inputs and which numerical allowance admit a relation such as `A(B+C) = AB+AC` under fp
arithmetic? A relation can be algebraically true and numerically misleading.

Required future work:

- relation-specific construction claims;
- source and correct-variant execution;
- adversarial input search;
- measured relation allowance;
- deliberately broken relation/candidate controls.

Do not present property strength as implemented until this is exercised on a real operator.

## OQ-007 — Multi-source disagreement policy

- Priority: P1
- Affects: source defect findings and admission outcome

When source behavior, reference, and external/upstream expectation disagree, when may Cairn localize
the defect and when must it stop as unresolved?

“Two against one” is not enough when two sources share code or derivation. The policy likely needs an
evidence-dependency graph and minimum independence rules. Create controlled correlated-failure
fixtures before defining automatic adjudication.

## OQ-009 — Target execution and runner attestation

- Priority: P2
- Affects: unverified verdict claims

What can Cairn independently prove about:

- the binary that ran;
- the image and libraries loaded;
- the physical/logical target device;
- whether candidate code reached the device;
- the worker binary and host policy?

Baseline: record observable identities and keep `device_execution`/`runner_attestation` unverified
until an independent mechanism exists. Do not make public release depend on hardware attestation.

## OQ-010 — Performance claims

- Priority: P2
- Affects: product result beyond correctness

Performance is now a first-class, non-compensating validation plane. Cairn owns operator-level
performance evidence, conditional roofline analysis, and performance admission, while the upstream
caller retains model-level business acceptance authority.

Questions:

- which operator-level claims and release thresholds the first caller requests;
- which production/simple/algorithmic baseline is primary for each claim;
- the first workload corpus and model-derived weighting policy;
- the minimum empirical evidence for `NearApplicableRoof` and `BottleneckSupported`.

The lifecycle and measurement principles are resolved in
[`oracle/PERFORMANCE_ORACLE_DESIGN.md`](oracle/PERFORMANCE_ORACLE_DESIGN.md); the concrete first
profile is OQ-020.

## OQ-011 — Public App Server transport

- Priority: P2
- Affects: client SDK and deployment

Stdio is ideal for local embedding; remote use needs a supported authenticated transport. Options:
WebSocket, gRPC streaming, or JSON-RPC over another framing. The stable resource model should be
designed before the network transport is frozen.

Baseline: implement typed in-process channels and stdio first; use generated schemas; delay a public
network compatibility promise until reconnect/backpressure behavior is measured.

## OQ-014 — External extension packaging

- Priority: P3
- Affects: open-source ecosystem

The design selects process-boundary extensions over native Rust dynamic ABI. Package manifest,
discovery, signing, permission prompts, and compatibility are not yet designed.

Trigger: two real out-of-tree provider/domain integrations whose duplication demonstrates the
contract. Until then, examples may be source workspace members.

## OQ-015 — Confidential source and evidence export

- Priority: P2
- Affects: data-boundary policy and open-source adoption

How should a task remain auditable when source bytes cannot be exported or sent to a model provider?
Potential approaches include local-only providers, encrypted/private CAS, redacted manifests, and
verifier access policies. A redacted export must state exactly which claims cannot be independently
reconstructed.

## OQ-016 — Historical fixture curation

- Priority: P0
- Affects: rewrite controls

Which old Cairn/Alloyport records and artifacts become checked-in public fixtures, and which contain
private code, provider data, absolute paths, or deployment secrets?

Required set by behavior:

- false correctness verdict and measured-family fix;
- per-case mutation blind spots;
- complete model-input audit and missing-input examples;
- recorded replay and same-input live divergence;
- recoverable wrong citation;
- output-capture failure;
- stale lease and duplicate worker controls;
- one complete end-to-end identity graph.

Sanitize by producing new explicit fixtures, not by editing historical evidence and pretending its
digest is unchanged.

## OQ-017 — Contribution policy and governance

- Priority: P1 before public release
- Affects: contributor expectations and downstream adoption

The outbound project license is MIT under
[`D-006`](DECISIONS.md#d-006--mit-license-for-the-public-project). Confirm contributor policy, DCO
vs CLA, governance/maintainer model, trademark/project-name policy, and the dependency license gate
before the first release. These controls must also address compatible provenance for imported code,
fixtures, corpora, model outputs, and vendor integrations.

## OQ-019 — First Intent Admission policy and corpus

- Priority: P0 before implementation resumes
- Affects: first `MigrationIntentContract`, SIR evaluation, downstream Oracle authority

Which exact claim set is the minimum admitted intent for the first kernel: mathematical operation,
ABI/shape relation, numerical mode, deployment specialization, side effects, and optimization
freedom? Which frozen cases distinguish implementation artifacts, source bugs, model-dependent quirks,
competing plausible meanings, and genuine unknowns?

The architecture fixes that SIR is proposal-only and admission is claim-scoped. It does not yet fix
the first operator, minimum hidden corpus, or which conflicts require a user decision. Those choices
must be made before defining V1 production intent artifacts.

## OQ-020 — First Ascend hardware-performance profile

- Priority: P1 before performance implementation
- Affects: hardware-fact schema, microbench registry, profiler adapter, performance admission

Select the first Ascend SoC, exact CANN/compiler/firmware environment, required computation and
memory ceilings, profiler fields, device-state controls, and production baseline. The architecture
requires conditional ceilings and calibrated measurements but intentionally does not invent numbers
or tool fields before the real environment is selected and probed.

## OQ-021 — Initial knowledge and skill admission profiles

- Priority: P1 before SIR/Oracle knowledge retrieval is enabled
- Affects: role capabilities, retrieval results, claim promotion, skill execution

Which claim kinds may be admitted from official documentation alone, which require execution
receipts, and which can only guide exploration? Which reviewed-but-unvalidated skills are allowed for
SIR and Blue, what sandbox capabilities may they request, and what exact evidence promotes each skill
claim to validated?

The T0–T3 and skill lifecycle are fixed; the per-claim/per-role minimum profiles remain policy work.

## OQ-022 — Real-model feedback acquisition and attribution

- Priority: P1 before model-integration feedback is enabled
- Affects: privacy, workload weighting, first-divergence, revalidation

Define the minimum model/deployment context Cairn may receive, whether activations/weights remain
external references, how representative workload weights are derived, and which first-divergence or
ablation evidence is required before a model-level regression is attributed to the migrated kernel.
Positive feedback cannot prove local correctness and negative feedback cannot silently rewrite the
Oracle; the unresolved question is the first concrete acquisition and attribution policy.

## OQ-023 — First verifier-mechanism qualification profile

- Priority: P0 before new Oracle Admission implementation
- Affects: comparator, adapter, runner, gate, policy evaluator, diagnostic redaction

Select the exact mechanisms in the first `VerificationMechanismSet`, their independent golden or
property oracles, required mutation/fault controls, real-tool calibration, reviewers, and
qualification/requalification policy. The architecture requires qualification but cannot use the
future gate to manufacture its own trust root.

## OQ-024 — Hidden corpus exposure and replenishment policy

- Priority: P1 before adaptive Blue/Candidate admission
- Affects: diagnostic utility, hidden strength, corpus cost, anti-overfitting

Define what diagnostic detail burns a case, whether exposure is tracked per proposal, model episode,
model family, task lineage, or globally, how many adaptive queries are allowed, and how a depleted
coverage partition is replenished and independently reviewed. Until decided, a revealed
counterexample is conservatively public for its applicant lineage.

## OQ-025 — First statistical/stateful Oracle policy

- Priority: P2; blocks only a random, stateful, or schedule-set first operator
- Affects: repetitions, reset, statistical power, legal outcome sets, replay

Choose supported RNG/state models, distributional claims, type-I/type-II error bounds, multiple-test
policy, minimum effect, repetition/reset isolation, and inconclusive outcome. Deterministic operator
slices may proceed without solving this, but they must not create a generic statistical default.
