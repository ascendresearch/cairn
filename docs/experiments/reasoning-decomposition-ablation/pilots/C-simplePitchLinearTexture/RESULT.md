# C pilot: `EvidenceAugmentedStructuredReview` on `simplePitchLinearTexture`

## Status and interpretation

This is an implementation pilot under `PILOT_PROTOCOL.md`, not a formal causal observation. It ran
through the normal CLI/server/application path with a real runtime model and an ordinary local
Worker. The operator interrupted it in the first Oracle dimension after the second item reached
revision 2 but before that revision's independent Review returned. It did **not** reach portfolio
coherence Review, Oracle controls, Oracle Admission or `OracleAccepted`.

The interruption followed the frozen stopping rule for an evidently invalid or unaffordable path:
the first dimension alone expanded into 12 partly overlapping items, the item-set Reviewer approved
the set without findings, and processing only two items had already consumed more model output than
the complete A pilot. The pending Reviewer dispatch was allowed about five minutes after its last
source read before the server was stopped. No approval is inferred from that interrupted call.

## Excluded preflights and fixes

Two clean-state C preflights found application defects. They are excluded from every measured C count
below, but retained here because they are part of the implementation result.

1. `/tmp/cairn-ablation-c-preflight-worker-mount-and-slot`, task
   `task:01a05d81-9142-7ee1-a4c1-44c6a7c67e18`, proved that the runtime model could request an
   experiment and the Controller could schedule it on an ordinary Worker. The job produced receipt
   `cairn:v1:sha256:execution.receipt.v1:e2f88cbb81cd3e7c2e970268f6414d6e7a6bd4958df80b6de789b76b1c77d51f`
   with exit code 2 because the trusted wrapper ran from `/cairn/work` but attempted
   `cd ../task`. The wrapper now uses the absolute mounted paths `/cairn/input/task` and
   `/cairn/input/experiment/program.sh`. An immediate second request then found no eligible Worker,
   exposing that a terminal execution reservation still occupied the sole Worker slot. Scheduling
   retries were added, but retrying alone could not repair an unreleased reservation.
2. `/tmp/cairn-ablation-c-preflight-language-and-reservation`, task
   `task:01a05d87-95d1-7a12-90a7-27749a24a737`, exercised the corrected mount. The model supplied
   Python source to an implicitly shell-only request, producing receipt
   `cairn:v1:sha256:execution.receipt.v1:a5ed6ac0be4c78a80d7b0442801022af7958b53e700ce6b7c9ab17ba8d9f2074`
   with exit code 2 (`import: not found` and a shell syntax error). The V1 request now requires the
   strong enum value `language: "posix-shell"`; both the role instruction and native tool
   description explicitly say POSIX `/bin/sh`, not Python, and promise neither a CUDA compiler nor
   a GPU. The run also recorded 113 unsuccessful placement attempts for a second request, proving
   that the completed reservation had not been released. The evidence runner now releases the
   reservation through the normal execution API and requires
   `ReservationReleaseReason::ExecutionTerminal`. The same latent sequential-slot defect was fixed
   in the Oracle control runner. Scheduling retries are rate-limited to at least one second and
   remain bounded by the configured completion deadline.

The measured pilot began in a new state directory after all of these fixes.

## Frozen identities and environment

- task: `task:01a05d8f-98bf-7362-9021-d81cdc60e6d1`
- treatment: `evidence-augmented-structured-review`
- source: CUDA Samples `simplePitchLinearTexture`, entry point `shiftPitchLinear`
- source-file SHA-256: `3198457e1fa97db64cbb0d67de690e75efbe59a8a70db8aebd15aa006b0b742b`
- target context: Ascend 950PR (3510), CANN 9.1.0-beta.1 Ascend C
- model alias: `deepseek-v4-pro`
- model-template SHA-256: `9f1ccc43aecd069f218512424d3702b2da9a790b71a803f2fe10893533affb09`
- server binary SHA-256: `138cb818a5375d2e2d331e89b1b7544a7b84e0543f8ee0216449d0b4e3e7fcc3`
- CLI binary SHA-256: `d3913d11584353443d7ab3a61064005920d77e4027736ea1c0ec2004c7da140b`
- Worker binary SHA-256: `1f4ae9c1510584eb65b808b1d77cc954a3c4358f494e27fc271e71545c49bcc2`
- runtime state: `/tmp/cairn-ablation-c-pilot`
- application accepted the task at `2026-09-01T15:21:26.987086Z`
- operator interruption began at `2026-09-01T15:58:53.068518Z`
- wall time between those events: about 2,246.1 seconds

A used an earlier server binary because the two Worker-path preflights and their fixes occurred
after A. The CLI and Worker hashes match. This code-identity difference is another reason these
pilots cannot be treated as a paired causal estimate.

The Worker was enrolled in pool `local-oracle`. Its experiment configuration required no
additional capability labels; accordingly, the runtime was explicitly told not to claim CUDA/GPU
availability. Every job used network-disabled execution, a frozen task-file bundle, a 32-KiB
program bound, and separate 16-KiB stdout/stderr capture bounds.

## Runtime path and measured work

The observed path was:

`cairn-cli -> cairn-server -> migration app API -> CudaMigrationWorkflow -> SIR Agent Loop ->`
`administrator decisions -> Intent Admission -> dimension discovery -> item-set Review ->`
`item development -> item Review -> item revision -> interrupted re-Review`.

- Agent Loop episodes: 10 opened, 8 completed normally, 2 incomplete
- completed episode steps: 37
- model dispatches: 44 started, 42 responses received, 1 definitively not sent, 1 active when
  interrupted
- provider usage from the 42 durable responses: 490,936 input tokens, 137,198 output tokens and
  376,576 cache-read tokens
- proposed tool calls: 50
- completed tool operations: 50
- task-artifact reads: 29
- model domain submissions: 8
- Worker experiment requests/jobs/successful receipts: 4/4/4
- Worker failures: 0

Per-role provider usage:

| Role | Episodes opened/completed | Responses | Input | Output | Cache read |
| --- | ---: | ---: | ---: | ---: | ---: |
| SIR analyst | 1/1 | 4 | 67,089 | 19,420 | 40,960 |
| Item discoverer | 1/1 | 5 | 70,424 | 13,760 | 59,264 |
| Item-set reviewer | 1/1 | 5 | 66,464 | 25,814 | 54,272 |
| Item developer | 3/3 | 13 | 142,885 | 39,028 | 110,976 |
| Item reviewer | 4/2 | 15 | 144,074 | 39,176 | 111,104 |

One item-Review episode ended after a DeepSeek request was definitively not sent due to an HTTPS
send error. The workflow waited and started a new reviewer attempt, which produced the actionable
revision decision. The other incomplete reviewer is the revision-2 re-Review interrupted by the
operator.

## SIR result

The SIR read the previously unseen task through five task-artifact calls and submitted a useful
hypothesis set without using a Worker experiment. It kept distinct:

- C remainder behavior demonstrated by the host gold;
- normalized point sampling with CUDA wrap semantics;
- the intended mathematical floor-modulo shift;
- the unresolved behavior of negative and large shifts and normalized-coordinate rounding.

It recorded 13 observed facts, 5 hypotheses, 2 conflicts, 3 unknowns, 3 invariants, 3 optimization
freedoms, 3 source dispositions and 2 proposed disambiguation experiments. The administrator
answered both decision requests with the same frozen authoritative floor-modulo claim used in A
and B, and Intent Admission committed both decisions. C therefore inherited the same duplicated
semantic claim under two distinct strong claim identities; this pilot reached only the first of the
resulting dimensions.

## Oracle result

The first dimension was `observable-semantics / observable-outputs`, identity
`cairn:v1:sha256:migration.oracle-dimension.v1:a14034f9307ff9677d52cc48911bdc4a0629b51a3d9812f36c4bce859bb37c20`.
Discovery requested two successful arithmetic experiments and proposed 12 items:

1. bit-exact subnormal and negative-zero preservation;
2. source/output buffer-bound safety;
3. the general exact floor-modulo mapping;
4. multiple-wrap shifts;
5. negative-x wrap;
6. negative-y wrap;
7. non-power-of-two mixed wrap;
8. pitched output placement;
9. positive-x boundary wrap;
10. positive-y boundary wrap;
11. repeatability;
12. unit dimensions.

The independent item-set Reviewer read the whole set and approved it with zero findings. Manual
audit disagrees: items 4--7 and 9--10 are concrete instances already entailed by item 3, while
repeatability and buffer safety partly cross concern boundaries. The concrete cases can be useful
as plans under a general item, but treating each as an independent item multiplies Developer and
Reviewer work. This is the strongest C quality failure observed in the pilot.

The first item, bit-exact preservation, produced three candidate-facing plans and was approved in
one draft/review round with no findings. The plans use an exact reference execution, candidate
source inspection, and a boundary probe contrasting tolerance comparison with raw-bit comparison.

The second item, buffer-bound safety, initially produced three candidate-facing plans. After one
provider-send failure and a fresh reviewer attempt, Review returned two `setup-incomplete`
findings:

- two adjacent canaries cannot detect writes landing farther than one element beyond the output
  region;
- the non-multiple shapes `W=257`, `H=129` with a rounded 16x16 launch introduce out-of-range
  threads, so the proposed run could reject an invalid launch rather than a valid-invocation
  violation.

The reviewer used a Worker arithmetic probe to substantiate both findings. Developer revision 2
replaced canaries as the primary mechanism with guard pages or a full target bounds checker,
selected tile-compatible launch shapes, made extra-thread suppression explicit, and used a fourth
Worker probe to check the revised arithmetic. A new reviewer read the exact revision and source,
but its next model call remained pending until operator interruption. Revision 2 is therefore
neither approved nor rejected.

## Evidence-experiment record

All four experiments ran through Controller scheduling on the same ordinary Worker, returned exact
receipt identity only to the requesting episode lineage, exited 0, and released their reservation
with `ExecutionTerminal`:

| Receipt suffix | Requesting role | Result bytes | Manual classification |
| --- | --- | ---: | --- |
| `ecdf96...bcda` | item discoverer | 559 stdout | useful transcription check for several concrete floor-modulo cases, but not independent semantic evidence |
| `128281...a97a` | item discoverer | 35 stdout | redundant subset of the first arithmetic check |
| `8f4310...d4af` | item reviewer | 240 stdout | actionable: demonstrated the invalid launch shape and adjacent-canary gap that caused revision |
| `94479b...043` | item developer | 379 stdout | useful repair check for tile-compatible shapes and in-bound offsets |

These were POSIX-shell integer calculations. None compiled or ran CUDA, none ran an Ascend-C
candidate on 950PR, and none carried Oracle Admission authority. They prove that C's typed
request/schedule/receipt/lineage loop works and can improve a local plan, not that the admitted
floor-modulo semantics or any candidate implementation is correct.

## Preserved artifacts and lifecycle outcome

The eight exact runtime-model submissions are in [submitted-artifacts](submitted-artifacts). The
one dimension, 12 item objects, nine check plans and one strategy-run object mechanically archived
by the workflow are in [derived-domain-artifacts](derived-domain-artifacts). Worker stdout/stderr,
native model continuations, prompts and credentials are deliberately not copied into the
repository; receipt identities, outcomes and bounded byte counts are recorded above.

The task remained in Oracle exploration when the server was stopped. Portfolio coherence Review,
qualified Oracle controls and mechanical Oracle Admission never ran. The C pilot therefore has no
terminal workflow completion and no semantic acceptance result.

## Conclusions retained from the pilot

1. C's experiment authority is genuinely product-path functionality: a runtime role can request a
   bounded observation, the Controller schedules an ordinary Worker, and only that episode receives
   the exact result.
2. Evidence access can materially improve Review. Here it exposed two concrete setup defects and
   led to a traceable revision rather than silent approval.
3. Evidence access does not itself improve decomposition. The discoverer used two calculations yet
   proposed 12 overlapping items, and item-set Review failed to catch the multiplicative overlap.
4. Not all experiments are valuable. One was redundant, and all four were arithmetic checks rather
   than CUDA, target-hardware or candidate observations.
5. Structured independent Review remains expensive. C consumed 137,198 output tokens with only one
   of 12 first-dimension items accepted and the second still pending, already over 2.5 times A's
   complete-pilot output.
6. The reservation-release and language failures were real product defects; the measured C pilot's four
   sequential successful jobs demonstrate that their fixes work on the normal path.
7. C provides positive evidence for a narrow evidence-to-revision mechanism, not evidence that C
   dominates A or B. A formal comparison still needs identical code, randomized repetitions, a
   common hidden semantic evaluator and hardware-capable treatments where the hypothesis requires
   CUDA or 950PR observations.
