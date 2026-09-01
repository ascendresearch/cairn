# A/C pilot protocol

This protocol is frozen before the first A or C execution. It governs implementation-pilot runs,
not the later formal causal comparison.

## Purpose

The existing B run proved useful enough to preserve but is not a reproducible formal observation.
The immediate A/C pilots answer narrower questions:

1. can each treatment traverse the normal production entry without a test-only proposal path;
2. does its runtime model genuinely read the same previously unseen task and submit typed artifacts;
3. what qualitative omissions, duplication, Review findings and experiment requests occur;
4. do all unsupported semantic qualification paths fail closed rather than publish false success?

No claim that A, B or C is superior may be made from these three single, non-contemporaneous pilots.

## Frozen task

- Corpus item: CUDA Samples `simplePitchLinearTexture`
- Source root: `/tmp/cairn-cuda-samples/cpp/0_Introduction/simplePitchLinearTexture`
- Entry point: `shiftPitchLinear`
- Target: Ascend 950PR (3510)
- Toolchain context: CANN 9.1.0-beta.1 Ascend C
- Environment context: `ascend-c-operator`
- Administrator decision: exactly the same authoritative claim recorded in the B pilot
- Completion target: through Oracle Admission
- Coverage: correctness, no required adversarial strategy role
- Candidate stage: not entered

## Frozen generous limits

The A and C pilots use these limits from their first attempt; they will not be increased in response
to individual model behavior:

- task files: 32;
- task bytes: 262,144;
- one task read: 2,048 lines and 262,144 UTF-8 bytes;
- episode steps: 128;
- tool operations per episode: 256;
- model output tokens per response: 131,072;
- role attempts: 8;
- provider token total: uncapped for natural closure;
- deadline: none.

These differ from B's 200-line/32-KiB read limit and are another reason B cannot enter a numerical
paired estimate.

## Treatment isolation

### A

- one SIR Agent Loop;
- normal administrator Intent Admission;
- one whole-portfolio Oracle proposal Agent Loop;
- no model Reviewer and no proposal-visible new Worker experiment;
- normal mechanical Oracle qualification/Admission boundary.

### C

- the same structured discovery, Review and revision topology as B;
- SIR and Oracle roles may request typed experiments;
- only Controller-authorized requests run on an ordinary capability-matched Worker;
- exact receipts return only to the requesting Agent Loop lineage;
- no proposal role receives hidden evaluator material.

## Common observation and stopping rules

For both arms record task/run identities, exact code/config/model aliases, all domain submissions,
episode/tool/token/time totals, typed rejections, manual quality findings and lifecycle outcome.

The run stops when one of the following occurs:

- Oracle Admission grants authority through a real semantic evaluator;
- the workflow reaches the explicit `OracleMechanisms` fail-closed boundary;
- a typed terminal budget/protocol failure occurs;
- the user or operator interrupts an evidently invalid or unsafe path.

Structural JSON validation is not semantic evaluation. Until a common hidden evaluator actually
executes candidate-facing mechanisms, reaching `OracleMechanisms` is the correct safe outcome and
must not be relabeled `OracleAccepted`.

## Manual pilot rubric

Manual review records, without feeding the labels back to the proposal Agent:

- supported SIR fact and obligation recall;
- unresolved conflicts and unjustified promotions;
- candidate-facing Oracle item ratio;
- cross-item and cross-dimension duplication;
- executable versus prose-only plans;
- independently supported versus source-self-referential evidence;
- new Reviewer findings by class;
- useful versus tautological experiment requests;
- total model and Worker cost/time;
- exact stage and reason for non-completion.

The formal experiment will replace this manual-only endpoint with one frozen hidden evaluator and
multiple randomized repetitions of all three arms.
