# A pilot completeness audit

This audit was performed after copying the result and before starting C.

## Preserved

- exact task/treatment/model/binary identities and runtime timestamps;
- both excluded preflight blockers and the product fixes they caused;
- normal production submission and administrator-decision path;
- episode, step, model, token, tool, Worker and lifecycle totals;
- the one typed SIR rejection and successful correction;
- exact submitted SIR and whole-portfolio content blobs;
- all 90 mechanically derived Oracle drafts, plans and accepted-item artifacts;
- manual strengths, duplication, method-label weakness and safe terminal outcome;
- explicit warning that this is neither `OracleAccepted` nor a formal causal result.

## Deliberately not preserved

- model chain-of-thought/native continuation state;
- full materialized prompts or model responses;
- credentials, enrollment bundles, TLS keys or environment variables;
- server/Worker stdout beyond aggregate facts reported in `RESULT.md`;
- the source tree itself, which remains identified by the frozen protocol.

The 92 retained JSON files all pass `jq empty`. Their contents are domain artifacts only. A scan for
common credential markers is required again in the final cross-arm audit.

## Known limitations

- The pilot has no common hidden semantic evaluator, so manual quality findings cannot establish
  correctness.
- B used different limits and an earlier implementation; B remains qualitative context only.
- The two administrator requests received semantically identical claims under distinct strong claim
  identities. The resulting 15 paired semantic dimensions expose a real duplication behavior but
  should not be confused with 30 independent requirements.
- Provider usage is reported from durable response events; wall time includes administrator delay
  and orchestration overhead.
