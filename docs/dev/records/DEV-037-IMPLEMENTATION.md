# DEV-037: Oracle dogfood fail-closed audit

- Date: 2026-09-01
- Status: implementation gap confirmed; unsafe success path closed
- Scope: normal CLI/server migration workflow through Oracle development

## What the live run proved

The non-`vectorAdd` `simplePitchLinearTexture` task entered through the normal
`cairn-cli -> cairn-server -> migration app API -> CudaMigrationWorkflow` path. A runtime model read
the previously unseen task, produced SIR and Oracle artifacts, consumed administrator intent
authority, and completed multiple independent item development/review/revision Agent Loops.
Reviewer findings caught real defects including swapped row/column indices, missing executable
observations, invalid launch assumptions and tautological metamorphic comparisons.

The durable task is
`task:01a05c3c-6b70-7b42-81f1-fb2086fdc2a2`. The local state remains under
`/tmp/cairn-dogfood-v3`; stopping the server did not delete it.

## Blocking findings

### 1. Coverage applicability is absent

The current correctness policy expands every admitted claim across fifteen mandatory concerns.
The live task therefore began developing both `observable-outputs` and
`allowed-result-relations` dimensions even though their generated item sets substantially
overlapped. The latter dimension proposed seven more items after the former had already accepted
three.

This contradicts the current design requirement that each concern be classified as `Required`,
`ApplicableInformational`, `NotApplicable` or `UnknownApplicability`. Local item-set Review cannot
remove cross-dimension duplication because it sees only one dimension, while portfolio coherence
runs only after every expensive item revision has completed.

### 2. Qualified controls are structural, not semantic

`OracleControlRunnerV1` sends a Worker script that verifies plan digest, item binding, schema,
method enumeration and non-empty prose fields. Honest and negative families mutate or validate the
plan JSON. The runner does not execute a check plan against an Ascend-C candidate, target receipt,
CUDA/reference provider or executable comparator.

Consequently the old path could turn a structural protocol self-check into semantic qualification.
That outcome would violate the current SIR/Oracle design and must not be reported as a successful
dogfood run.

## Changes in this slice

- `OracleItemSetReviewIssueClassV1` now has the distinct `NotCandidateFacing` outcome.
- Item discovery and item-set Review instructions require every item to judge a future Ascend-C
  candidate or target execution receipt. Restating Intent, characterizing only CUDA, or listing
  implementation freedom is explicitly insufficient.
- Item development and Review instructions require an obtainable candidate-facing observation and
  acceptance condition.
- The structural control runner now returns `SemanticExecutionUnavailable` before it can publish a
  qualified semantic mechanism catalog.
- That condition crosses the generic product-service boundary as the distinct
  `OracleSemanticMechanismUnavailable` workflow failure class. Product lifecycle state maps it to
  `OracleMechanisms` attention rather than erasing it into a generic workflow failure.
- The task-artifact read tool separately exposes its exact frozen line/byte bounds and actionable
  chunking guidance; this was implemented earlier in the same dogfood session after repeated
  rejected whole-file reads.

These are current-V1 edits. No compatibility reader, V2 format, fixture branch, known answer or
test-only proposal path was added.

## Required next implementation

1. Add a reviewed, claim-scoped concern-applicability phase before item development. Its typed
   outcome must preserve required, informational, not-applicable and unknown states, and global
   decomposition Review must see prior dimensions before expensive item loops begin.
2. Connect the existing durable external-effect yield to an Oracle experiment tool:
   `OracleExperimentRequestV1 -> Controller JobContract -> capability-matched ordinary Worker ->
   TrustedOracleWorkerReceiptV1 -> OracleExplorationObservationV1 -> same Agent Loop`.
3. Replace structural-only qualification with candidate-facing executable mechanisms. Admission
   must require honest/correct-variant observations and rejection of targeted mutant and hidden
   challenges; protocol structure checks may remain supplemental but cannot grant semantic
   authority.
4. Resume dogfood from a clean new task through the same normal entry. The preserved interrupted
   task is diagnostic lineage, not a baseline to reinterpret as successful.

## Verification

After the fail-closed changes:

- `cargo test -p cairn-migration -p cairn-migration-app --all-targets --no-fail-fast` passed;
- `cargo clippy -p cairn-migration -p cairn-migration-app --all-targets -- -D warnings` passed;
- the only ignored test remained the explicit opt-in live GitHub test.
