# B pilot: `simplePitchLinearTexture`

## Classification and outcome

- Treatment: B, `StructuredReview`
- Classification: incomplete pilot/debug run; not a formal ablation observation
- Task: `task:01a05c3c-6b70-7b42-81f1-fb2086fdc2a2`
- Source: `/tmp/cairn-cuda-samples/cpp/0_Introduction/simplePitchLinearTexture`
- Normal entry: `cairn-cli -> cairn-server -> migration app API -> CudaMigrationWorkflow`
- Target context: Ascend 950PR (3510), CANN 9.1.0-beta.1 Ascend C,
  `ascend-c-operator`
- Runtime model alias: `deepseek-v4-pro`
- Completion target: `through-oracle-admission`
- Coverage profile: correctness
- Adversarial policy: not required
- Final outcome: interrupted during the third item of the second Oracle dimension
- Oracle portfolio Review: not reached
- Oracle controls and Admission: not reached
- Candidate workflow: not reached by design

The task was stopped deliberately after code inspection proved that the then-current Oracle control
runner performed structural JSON validation rather than executing a candidate-facing Oracle. An
`OracleAccepted` result from that runner would have been invalid evidence.

The durable diagnostic state remains in `/tmp/cairn-dogfood-v3`. The proposal Agent's 33 formal
domain submissions are copied under `submitted-artifacts/`; private model reasoning, prompts,
credentials, stdout and stderr are intentionally not copied.

## Frozen input and administrator authority

The recovery request exposed one caller claim: preserve caller-visible `shiftPitchLinear` output
behavior for valid invocations. It declared the CUDA entry point and seven arguments, including the
output pitch in elements and the opaque texture handle. It also exposed three unresolved questions:

1. exact transformation, coordinate convention and boundary behavior;
2. which CUDA texture-resource details are semantic versus replaceable on Ascend C;
3. the required numerical comparison.

The administrator admitted the following exact algorithmic contract:

> For every valid logical output coordinate `(x,y)`, produce the exact binary32 input element at
> `(floor_mod(x + shiftX, width), floor_mod(y + shiftY, height))`. The result is an exact element
> permutation; CUDA normalized-coordinate floating-point rounding and the
> `cudaTextureObject_t` handle are replaceable implementation mechanisms rather than required
> observable semantics.

The admitted domain required positive width/height, writable output storage for `height` rows with
`pitch >= width`, a readable logical input region, and representable shifted-coordinate arithmetic.
This was a real administrator decision, not a fixture answer embedded in product code.

## SIR result

The SIR Agent read the previously unseen sample and submitted:

- 12 observed facts with exact source citations;
- 5 invariants;
- 4 competing hypotheses;
- 2 explicit conflicts;
- 3 unknowns;
- 1 disambiguation experiment;
- 1 optimization freedom;
- 4 source dispositions.

Important recovered facts included:

- the kernel writes `odata[yid * pitch + xid]` and receives pitch in elements;
- the pitch-linear texture uses normalized coordinates, point filtering and wrap on both axes;
- the bundled host reference uses positive modular shifts `x_shift=5`, `y_shift=7`;
- input and output are pitch-linear allocations;
- the bundled comparison uses `compareData(..., 0.0f, 0.15f)` even though the transformation is an
  element permutation.

The SIR correctly refused to collapse several distinctions:

- exact integer floor-modulo versus normalized-coordinate floating-point behavior at large
  magnitudes;
- full signed-shift support versus only the demonstrated nonnegative domain;
- CUDA texture handle as required interface versus replaceable deployment detail;
- exact bitwise output versus the sample harness tolerance.

It proposed a CUDA probe over negative, oversized and large-magnitude shifts. B could describe that
experiment but, by treatment definition, could not request or consume a new Worker observation.

The complete submitted SIR is [01-sir.json](submitted-artifacts/01-sir.json).

## Oracle progress

The policy mechanically expanded the admitted claim across the fixed correctness concern inventory.
The pilot completed only the first dimension and part of the second.

### Dimension 1: observable semantics / observable outputs

Dimension identity:
`cairn:v1:sha256:migration.oracle-dimension.v1:08308a44c58c58302f039a3438d7173c23150dfdca17ac122b298a80dd5115a4`

The first discovery proposed four items. Item-set Review rejected the decomposition because the
standalone exact-copy item was subsumed by the full elementwise mapping item. The revised three-item
set was approved:

1. Exact candidate output mapping and bit-preserving binary32 copy for every logical coordinate.
2. Output scope: logical columns only; output padding and CUDA implementation mechanisms are not
   observable requirements.
3. Mathematical floor-modulo definition, including negative shifted coordinates and rejection of
   clamp/zero-fill behavior.

All three items were eventually accepted:

| Item | Draft/review rounds | Reviewer findings before acceptance |
| --- | ---: | --- |
| Exact elementwise mapping | 3 | missing output allocation; then source input not actually passed in two execution plans |
| Observable-output scope | 4 | underdetermined metamorphic relation; padding probe might have no padding; missing source/input binding and output pitch; composed run did not rebind its input; remaining circular metamorphic anchors; missing small-shape launch coverage |
| Floor-modulo behavior | 3 | tautological metamorphic test; missing citation for concrete shifts; requested `C(W,H)` observation was not generated; row/column range wording was reversed |

The accepted item identities were:

- `cairn:v1:sha256:migration.oracle-item.v1:ab18082e462f316062edcc534e0ad62ef8a96927a1f17e63354e121cbae83169`
- `cairn:v1:sha256:migration.oracle-item.v1:06860ac90dce3a412a3cd4f74bba2aef9d583de70b5a6bb09e48ae6b0f1725c3`
- `cairn:v1:sha256:migration.oracle-item.v1:2b2c173f615c256a5b7dc22aa1094370bcf7394d8e69335f851dd7328d8a9e47`

### Dimension 2: observable semantics / allowed-result relations

Dimension identity:
`cairn:v1:sha256:migration.oracle-dimension.v1:539ae401ab3e6a6c68eb5c720f6fbec652cb2d905eb1ba1b28522945b6c045d8`

Discovery proposed seven items and local item-set Review approved them without revision:

1. scope of observable output;
2. exact elementwise floor-modulo result relation;
3. logical input-source relation and exclusion of input padding;
4. mathematical floor-modulo index mapping;
5. valid-domain boundary;
6. implementation freedom;
7. exhaustion of the relation, excluding sample timing, bandwidth and console output.

This set substantially duplicated Dimension 1. In particular, exact element mapping,
floor-modulo, output/padding scope and implementation freedom were already developed and accepted.
Because the item-set Reviewer saw only its exact dimension, it could not detect the cross-dimension
duplication.

Progress within this dimension:

- Exact elementwise relation was accepted after two rounds. Review caught swapped row-major
  concrete examples: the correct indices were `4*7+5` and `3*7+4`, not `5*7+4` and `4*7+3`.
- Implementation freedom was accepted in one round, but its four plans were static analyses of the
  CUDA source/Intent and did not consume a future Ascend-C candidate. Manual audit therefore marks
  this accepted result as **not candidate-facing** under the corrected criterion.
- Floor-modulo index mapping opened its first Developer episode and read its exact context, but the
  model call was interrupted before a draft was submitted.
- The remaining four items never entered development.

Accepted item identities:

- `cairn:v1:sha256:migration.oracle-item.v1:acda852e8f4b2b83e40c63f81e7226f91715d8d9e1c80eaf672a34bdb07fc6d1`
- `cairn:v1:sha256:migration.oracle-item.v1:6727302167fde27b4059f4e91b63ae4e10cef6c956468559b90c604abe5de70f`

Interrupted item identity:
`cairn:v1:sha256:migration.oracle-item.v1:a542385a01bc798f5f0ef9178e0087e4ad9009d6ecb31fbf9d920a1a6ac9220a`.

## Quantitative execution record

Time range is derived from durable event timestamps.

- First B episode opened: epoch ms `1788253858697` (2026-09-01 09:10:58 UTC)
- Last event before interruption: epoch ms `1788261323041` (about 2 h 4 min later)
- Agent episodes: 34 opened, 33 completed, 1 incomplete
- Model dispatches: 167 started, 166 responses received
- Tool calls proposed: 208
- Tool operations completed: 192
- Tool operations rejected: 16
- Input tokens reported: 1,744,320
- Output tokens reported: 493,434
- Cache-read tokens reported: 1,404,160

Per-role totals:

| Role | Episodes | Responses | Input tokens | Output tokens | Cache-read tokens | Completed episode seconds |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| SIR analyst | 1 | 5 | 79,452 | 22,450 | 58,368 | 309.9 |
| Item discoverer | 3 | 16 | 115,246 | 19,585 | 87,168 | 297.8 |
| Item-set reviewer | 3 | 14 | 131,237 | 37,682 | 103,936 | 548.5 |
| Item developer | 14 | 65 | 603,460 | 145,629 | 478,080 | 1,911.3 |
| Item reviewer | 13 | 66 | 814,925 | 268,088 | 676,608 | 3,881.6 |

Reviewer cost dominated the observed run: item Review consumed about 54% of output tokens and 56%
of completed episode time. This is a pilot observation, not yet an effect estimate.

The 13 completed item reviews produced 16 actionable findings:

- 11 `setup-incomplete`;
- 3 `objective-incomplete`;
- 1 `unsupported-evidence`;
- 1 `pass-condition-ambiguous`.

There was also one item-set `overlapping-items` finding. The findings were substantive rather than
mere rewriting: they identified missing buffers/bindings, an invalid zero-grid small-shape setup,
tautological or circular metamorphic checks, missing evidence, an unobtainable observation and
incorrect row-major indices.

## Tool behavior and protocol deviations

The actual task limits were:

- maximum files: 32;
- maximum task bytes: 262,144;
- maximum read lines per call: 200;
- maximum read bytes per call: 32,768;
- episode step limit: 64;
- tool-operation limit: 128;
- model output-token limit: 131,072;
- migration role-attempt limit: 8.

The runtime made 141 task-artifact read calls. Ten operations were rejected for exceeding the task
read limit. Six more were rejected because the submitted JSON omitted `schema_version`. The old tool
description did not disclose the exact byte bound or provide sufficient chunking guidance; this was
fixed after the pilot and is therefore a treatment deviation, not a B result.

The pilot also predates the strengthened candidate-facing discovery/Review instructions. Those
instructions were changed only after manual audit found the accepted implementation-freedom item.

The exact executable binary identity was not recorded. Repository `HEAD` was `866aa21`, but the
worktree contained uncommitted implementation changes. This alone prevents treating the pilot as a
reproducible formal repetition.

## Worker and Admission facts

A normal local Worker was enrolled and emitted heartbeats, but this task submitted no Oracle
control job before interruption. No CUDA experiment requested by the proposal Agent ran, consistent
with B treatment. No Ascend-C candidate, correct variant, mutant or hidden challenge ran.

Subsequent source audit found that `OracleControlRunnerV1` would only have checked plan JSON schema,
digest and bindings. It would not have executed a plan against a candidate or target receipt. The
runner has since been changed to fail closed with `SemanticExecutionUnavailable`, propagated as the
strong workflow failure `OracleSemanticMechanismUnavailable` and `OracleMechanisms` attention.

## Conclusions retained from the pilot

1. The normal CLI/server/workflow path and real runtime-model SIR/Oracle Agent Loops were connected.
2. The model genuinely read an unfamiliar non-`vectorAdd` CUDA task and recovered useful competing
   semantic hypotheses rather than merely copying the sample's gold loop.
3. Independent Review added real value: it repeatedly found executable setup defects, evidence
   gaps, circular properties and incorrect concrete indices.
4. Review was also extremely expensive and sometimes needed several revisions to discover basic
   ABI/setup omissions. H1 may hold qualitatively, but H2 and cost concerns are strongly motivated.
5. Local per-dimension Review could not prevent substantial global duplication between
   `observable-outputs` and `allowed-result-relations`. This is direct pilot evidence relevant to H4.
6. A structurally approved item can still fail the candidate-facing criterion. Schema completion and
   model approval are not semantic qualification.
7. B could formulate a valuable CUDA disambiguation experiment but could not execute it. This is a
   concrete reason to test C rather than an assumption that more roles suffice.
8. The old control runner could not support any valid comparison because it measured JSON protocol
   conformance, not Oracle adequacy.
9. This pilot must not be compared numerically with future A/C runs. A/B/C formal repetitions need
   one frozen manifest, identical enlarged read/budget settings, identical code/prompt identities,
   randomized arm order and a common hidden evaluator.
