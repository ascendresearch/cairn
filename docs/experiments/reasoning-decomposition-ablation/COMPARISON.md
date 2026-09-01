# A/B/C pilot comparison

## Scope

This document compares three implementation pilots on the same CUDA Samples
`simplePitchLinearTexture` task. It is a preservation and design-learning artifact, not an effect
estimate. The runs were single, sequential, non-randomized observations; B used different limits
and an unidentified binary, and A/C used different server binaries. No arm had a common hidden
semantic evaluator.

## Descriptive execution record

| Arm | Runtime topology | Episodes opened/completed | Model responses | Input / output / cache-read tokens | Tool result | Worker jobs | Observed endpoint |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| A | SIR + one whole-portfolio proposal | 2/2 | 7 | 153,867 / 53,138 / 110,592 | 9 completed, 1 rejected | 0 | fail closed at `OracleMechanisms` after a 30-item portfolio; about 717.7 s |
| B | dimension/item discovery + independent Review/revision | 34/33 | 166 | 1,744,320 / 493,434 / 1,404,160 | 192 completed, 16 rejected | 0 | interrupted in dimension 2 after 5 accepted items; about 7,464.3 s |
| C | B topology + proposal-visible Worker evidence | 10/8 | 42 | 490,936 / 137,198 / 376,576 | 50 completed | 4/4 successful | interrupted in dimension 1 after 1 accepted item and 1 pending revision; about 2,246.1 s |

These totals are not normalized by completed semantic coverage. In particular, C's first dimension
contained 12 items, while B's first approved dimension contained three and A proposed the full
portfolio in one episode. Direct token-per-run comparisons would therefore mix treatment behavior,
stopping depth and code differences.

## What each pilot actually established

### A: breadth without independent criticism

A produced a broad, candidate-facing 30-item portfolio quickly relative to the structured arms and
failed closed correctly when no semantic execution mechanism existed. It also preserved two copies
of the same administrator claim under distinct strong identities, creating 30 structural dimensions
but only 15 semantic plane/concern combinations. Without item Review or coherence Review, duplicated
checks and some mislabeled methods survived untouched.

### B: criticism is valuable and expensive

B's item Review found real defects: missing buffers and bindings, circular metamorphic checks,
unavailable observations, unsupported evidence, a zero-grid setup and incorrect row-major indices.
It also required many revisions, dominated output-token and elapsed cost, accepted one
non-candidate-facing item, and could not see duplication across dimension-local scopes. B supports
the claim that decomposition focuses Review, but not the claim that more loops automatically produce
a globally better or affordable portfolio.

### C: evidence can repair a local plan, not the decomposition policy

C is the first pilot in which proposal roles caused real ordinary-Worker jobs. A reviewer used one
receipt to prove that a plan's launch and memory-canary setup were invalid, and Developer revision 2
addressed both findings. This is a concrete benefit absent from B. However, the discoverer's first
two experiments were reference arithmetic (one redundant), all four experiments lacked CUDA/GPU or
950PR execution, and the item-set Reviewer approved a 12-item set with obvious subsumption. C added
agency and local evidence, but it also exposed a new cost-amplification path.

## Cross-arm design conclusions

1. Keep the structured seams, but do not equate the number of items, roles or accepted JSON objects
   with trustworthiness. Trust must come from independent executable qualification against the
   future candidate and exact target contract.
2. Preserve independent item Review: B and C both show that it finds defects a Developer misses.
   Pair it with a much stronger set/coherence criterion that treats concrete examples as plans under
   a general property unless they represent a genuinely distinct failure mode.
3. Keep typed Worker experiments as optional evidence authority. Require each request to name the
   uncertainty it can discriminate, and score redundant/reference-only probes separately from CUDA,
   candidate and target-hardware observations.
4. Capability matters more than the existence of a Worker. A POSIX-shell Worker can validate
   arithmetic and harness logic but cannot settle CUDA texture semantics or qualify Ascend-C on
   950PR. Formal treatments must freeze the capability matrix and expose only truthful capabilities.
5. A portfolio-level semantic evaluator is indispensable. A failed local reviewer, duplicated
   strong claims and structural plan validation all demonstrate that model consensus is not
   Admission evidence.
6. Cost must be an outcome, not an afterthought. The formal experiment should report semantic
   defects found per evaluator-qualified item, duplicate coverage, model/Worker cost and time—not
   simply workflow completion.

## Requirements before a causal ablation

- freeze one commit/binary/config/prompt/model/capability manifest for all arms;
- fix one common hidden evaluator and mutation/challenge set that no proposal role can read;
- run multiple randomized repetitions rather than one arm in a fixed order;
- define semantic item equivalence and coverage before seeing arm outputs;
- distinguish source/reference arithmetic, CUDA observations, candidate executions and 950PR
  executions in the scoring rubric;
- use the same generous limits and the same operator stopping rule for every repetition;
- report fail-closed and interrupted runs without imputing missing approvals or acceptance.

The pilot evidence therefore justifies continuing the ablation, but not selecting A, B or C as the
product architecture yet.
