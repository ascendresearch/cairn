# A pilot: `MinimalDecomposition` on `simplePitchLinearTexture`

## Status and interpretation

This is an implementation pilot under `PILOT_PROTOCOL.md`, not a formal causal observation. It ran
through the normal CLI/server/application workflow with a real runtime model. It reached the frozen
safe stopping boundary and failed closed because no Ascend semantic evaluator was configured. It
did **not** reach Oracle Admission and must not be described as `OracleAccepted`.

Two earlier clean preflights found product blockers. Both preflights are excluded from the pilot
measurements below:

1. `task:01a05d50-0308-7451-adb8-2432ec7dffd6` showed that the whole-portfolio scope exposed
   dimension and claim values but omitted their strong identities while the submission contract
   required those identities. The scope now projects `{dimension_id, dimension}` and
   `{claim_id, claim}`, and expected dimensions use the same canonical ordering as the wire form.
2. `task:01a05d63-8a77-7082-a1a4-84dcfec13c09` submitted a correctly scoped portfolio, but the
   runtime assigned the nonexistent strategy name `model-backed-whole-portfolio`. The whole-
   portfolio and item paths now use the catalogued `model-backed-synthesis` strategy constant.

These were application defects rather than evidence about treatment A. The measured pilot started
from a new state directory after both fixes.

## Frozen identities and environment

- task: `task:01a05d71-e41e-7002-bbbf-779c0cd347cb`
- treatment: `minimal-decomposition`
- source: CUDA Samples `simplePitchLinearTexture`, entry point `shiftPitchLinear`
- target context: Ascend 950PR (3510), CANN 9.1.0-beta.1 Ascend C
- model alias: `deepseek-v4-pro`
- server binary SHA-256: `f3c9d4e43bb22c137aa3f47851a9d0bd0be706c16e1c7aa951fec31de75ab997`
- CLI binary SHA-256: `d3913d11584353443d7ab3a61064005920d77e4027736ea1c0ec2004c7da140b`
- Worker binary SHA-256: `1f4ae9c1510584eb65b808b1d77cc954a3c4358f494e27fc271e71545c49bcc2`
- runtime state: `/tmp/cairn-ablation-a-pilot`
- application accepted the task at `2026-09-01T14:49:00.201830Z`
- terminal fail-closed event: `2026-09-01T15:00:57.878866Z`
- wall time between those events: about 717.7 seconds

The ordinary Worker enrolled in pool `local-oracle`, but A exposed no proposal-visible experiment
tool and the run scheduled no Worker job. Worker activity consisted only of registration and 26
heartbeats.

## Runtime path and measured work

The observed path was:

`cairn-cli -> cairn-server -> migration app API -> CudaMigrationWorkflow -> SIR Agent Loop ->`
`administrator decisions -> Intent Admission -> whole-portfolio Oracle Agent Loop ->`
`OracleMechanisms fail-closed boundary`.

- Agent Loop episodes: 2, both completed normally
  - SIR analyst: 4 steps
  - whole-portfolio proposer: 3 steps
- model dispatches/responses: 7/7
- provider usage: 153,867 input tokens, 53,138 output tokens, 110,592 cache-read tokens
- proposed tool calls: 10
- completed tool calls: 9
- rejected tool calls: 1
- Worker experiment requests/jobs/receipts: 0/0/0

The single rejection was useful mechanical feedback: the first SIR submission cited a line outside
the source artifact. The same Agent Loop corrected it on its next model turn and submitted a valid
SIR. No budget, protocol, or authority boundary was relaxed.

## Submitted artifacts

The SIR submission contains:

- 13 observed facts;
- 5 competing hypotheses;
- 2 conflicts;
- 3 unknowns;
- 3 invariants;
- 3 optimization freedoms;
- 3 source dispositions;
- 2 proposed disambiguation experiments.

The administrator answered the two generated decision requests using the same frozen authoritative
claim used in B. Intent Admission committed both decisions. The whole-portfolio episode then
submitted exactly 30 dimension entries and 30 items, one per strong dimension identity. The
mechanical projection produced 30 revision-1 drafts, 30 check plans and 30 whole-episode accepted
items.

The exact model-submitted domains are in [submitted-artifacts](submitted-artifacts). Mechanically
projected V1 drafts, plans and accepted items are separately retained in
[derived-domain-artifacts](derived-domain-artifacts), so they cannot be mistaken for independent
Reviewer output.

## Manual quality review

The SIR is materially useful: its facts cite source lines, it separates the integer floor-modulo
interpretation from CUDA normalized-texture behavior, preserves the pitch/write-footprint
obligations, and explicitly records resource-mapping and boundary-semantics uncertainty. Its first
invalid citation was caught and repaired rather than silently accepted.

The Oracle portfolio is candidate-facing and mechanically complete:

- all 30 plans cite both source evidence and the admitted intent;
- all 30 plans name an observation over a future migration candidate;
- methods comprise 13 static analyses, 5 boundary probes, 4 metamorphic checks, 4 reference
  executions and 4 runtime observations;
- plans include bitwise reference checks, negative/large shifts, degenerate axes, padded-row
  sentinels, special binary32 bit patterns, determinism, inverse-shift metamorphism, interface and
  dependency inspection.

The largest quality defect is duplication. The two administrator decisions instantiate two
distinct strong claim identities carrying the same authoritative claim. The policy therefore
creates 30 strong dimensions but only 15 semantic plane/concern combinations. Without item Review
or portfolio coherence Review, A preserves all 30 and produces near-paired repetitions: two error-
status inspections, two CUDA-dependency inspections, multiple padding-sentinel checks, multiple
special-value checks, and overlapping negative-shift/boundary probes. The repetitions are not
byte-identical, but many do not add enough independent coverage to justify their cost.

A second weakness is that `static-analysis` plans often mix inspection with a runtime assertion in
their pass condition. They remain executable as multi-part checks, but the method label understates
the required mechanism. A Reviewer would likely split or relabel several of them.

Conversely, the portfolio did not merely restate source behavior: every plan targets the future
Ascend-C candidate, uses explicit setup/observation/pass conditions, and most runtime plans specify
an independent host floor-modulo oracle. The run therefore demonstrates that minimal decomposition
can generate a broad candidate-facing portfolio, but not that the portfolio is semantically sound
or cost-efficient.

## Lifecycle outcome

The whole-portfolio submission passed current structural and authority checks with 30/30 exact
dimension identities. Cairn then entered `AwaitingOracleControls` and committed:

- failure class: `OracleSemanticMechanismUnavailable`;
- operator attention: `OracleMechanisms`;
- application error class: `oracle-semantic-mechanism-unavailable`.

That outcome is correct. The local Worker could execute generic containers, but no qualified Ascend
950PR semantic mechanism existed for these candidate-facing plans. Structural acceptance of a plan
is not evidence that its checks passed.
