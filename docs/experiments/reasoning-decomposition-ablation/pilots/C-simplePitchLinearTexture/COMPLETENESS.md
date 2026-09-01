# C pilot completeness audit

This audit was performed after stopping C and before writing the cross-arm comparison.

## Preserved

- exact task, treatment, source, target, model, binary and runtime-state identities;
- both excluded preflights, their task/receipt identities, observed failure modes and product fixes;
- normal CLI/server/application/Worker route and exact lifecycle stopping point;
- episode, dispatch, response, token, tool, Worker and elapsed-time totals;
- the provider `NotSent` event, fresh-reviewer retry and operator-interrupted dispatch;
- exact SIR, item-set, item-set Review, three draft and two item-Review submissions in chronological
  order;
- all 23 mechanically archived dimension, item, plan and strategy-run domain objects;
- all four Worker receipt identities, outcomes, exit codes, stdout byte counts, roles and manual
  usefulness classifications;
- manual SIR strengths, item-set overlap failure, item-level Review findings, revision changes and
  the explicit absence of portfolio Review, controls and Admission;
- explicit warning that no experiment ran CUDA, Ascend-C or Ascend 950PR and that the pilot is not a
  causal comparison.

## Deliberately not preserved

- model chain-of-thought/native continuation state;
- full materialized prompts or model responses;
- credentials, enrollment bundles, TLS keys or environment variables;
- raw Worker stdout/stderr or experiment programs; their safe aggregate facts and receipt identities
  are sufficient for this report;
- the `/tmp` runtime databases or source tree, which remain identified by frozen paths and digests.

The 31 retained JSON files are current-V1 product domain artifacts. They must all pass `jq empty`.
A common credential-marker scan and JSON count check are part of the final cross-arm audit.

## Known limitations

- C was interrupted in the first dimension and has no portfolio-level or Admission outcome.
- Revision 2 was submitted but its independent Review was interrupted; it is not accepted.
- The item-set approval conflicts with manual review and shows that one runtime reviewer is not a
  semantic ground truth.
- All four experiments were shell arithmetic checks on a Worker with no CUDA/GPU capability
  promise; they do not answer the SIR's CUDA semantic ambiguities.
- A and C used different server binaries because C preflights exposed Worker-path defects after A.
- B used different limits and an earlier unknown binary. Numerical A/B/C comparisons are descriptive
  pilot facts only.
