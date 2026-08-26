# Oracle Agent dogfood

This is the acceptance path for the actual Blue/Red product loop, not a replacement for offline
conformance tests. Recorded providers make the loop reproducible; opt-in live providers expose wire,
prompt, cache, provider, and orchestration defects that recorded responses cannot reveal.

## Configuration

[`config/oracle-blue-dogfood.example.json`](../config/oracle-blue-dogfood.example.json) selects the
recorded research provider. Blue and Red have independent limits for:

- durable model turns;
- admitted logical tool operations;
- cumulative provider tokens;
- provider output tokens per turn.

Research configuration separately selects the provider, sorted repository allowlist, maximum
results per search, and maximum provider response bytes. The limits are trusted configuration and
remain unavailable to the model. Later Red/admission stages must consume the same role budgets
rather than introduce hidden constants.

For live GitHub research, use
[`config/oracle-blue-dogfood.live-github.example.json`](../config/oracle-blue-dogfood.live-github.example.json).
Put the raw token, one line with no quotes, at:

```text
.cairn/secrets/github-token
```

and run `chmod 600 .cairn/secrets/github-token`. The JSON stores only this secret-file reference;
the token must not enter prompts, events, CAS, logs, or command-line arguments.

The current adapter calls `GET /search/code` and immutable Git blob endpoints. GitHub's current
fine-grained-PAT endpoint list does not include code search, although GitHub App installation tokens
do support it. For research over third-party public repositories such as `pytorch/pytorch`, use a
dedicated classic PAT with no repository scopes; it supplies authentication for public data without
granting private-repository access or writes. For repositories on which an app can be installed,
prefer an installation token restricted to selected repositories with read-only Contents/Metadata.
Do not grant issues, pull requests, actions, packages, administration, or write permissions.

References: [GitHub fine-grained PAT endpoint list](https://docs.github.com/en/rest/authentication/endpoints-available-for-fine-grained-personal-access-tokens),
[GitHub App installation-token endpoint list](https://docs.github.com/en/rest/authentication/endpoints-available-for-github-app-installation-access-tokens),
and [REST authentication guidance](https://docs.github.com/en/rest/authentication/authenticating-to-the-rest-api).

## Running the current gate

The model credential remains at `.cairn/secrets/deepseek-api-key`, as documented in the root
README. The recorded-research live-model gate is:

```bash
cargo run -p cairn-migration --example oracle_blue_research_live -- \
  config/oracle-blue-dogfood.example.json
```

After installing the GitHub token, replace the last argument with the live-GitHub example. An
optional second argument selects one semantic sample:

```bash
cargo run -p cairn-migration --example oracle_blue_research_live -- \
  config/oracle-blue-dogfood.live-github.example.json sum-noncontiguous
```

The current matrix names are `sum-empty-axis`, `max-empty-axis`, `sum-noncontiguous`, `sum-nan`,
and `matmul-zero-k`. Each run creates a fresh Blue episode and a distinct Red episode. It prints the
model's final typed Blue draft and Red review, their content identities, provider/cache usage, and
closure booleans. It never prints credentials, provider reasoning, or fetched source snippets. The
final submitted draft/review must be visible because this gate evaluates semantic quality, not just
connectivity.

## Dogfood ledger

The first live Blue run on 2026-08-26 found two defects before completing:

1. the role plan derived a tool-catalog identity but no product API archived its bytes, so generic
   input audit rejected the request as missing content;
2. all `oracle.*` tool names violated the live Responses provider's portable name pattern.

V1 now has an explicit catalog archival operation, retains transport rejection diagnostics, and
uses portable underscore tool names. The next run completed real model search selection, durable
recorded PyTorch research execution, native continuation recovery with byte-identical restart, and
a second synthesis turn. Provider usage reported 0 cached tokens on the first turn and 896 cache-read
tokens on the second turn. No license endpoint was queried and research bytes acquired no corpus
case identity.

The first live-GitHub run then succeeded technically but sent complete matched source files back to
the model. Its second turn consumed 82,287 input tokens. V1 now archives each full immutable blob
and exact research result for reconstruction, but returns only a deterministic query-centered,
line-addressed excerpt capped at 4 KiB per match. With the same three-result query, the second turn
fell to 3,112 input tokens while retaining exact source/result identities.

The first semantic answer was still wrong: it proposed shape `[0,3]` reduced on dimension 1, which
has three elements per conceptual row and zero output cells, then claimed to test empty-sum identity.
The result was vacuously true. The matrix now uses `[2,0,3]` reduced on dimension 1, producing six
observable identity cells, and adds four distinct failure surfaces: rejection without an identity,
strided indexing, NaN propagation, and zero-length inner products.

Further dogfood found and fixed two harness/configuration assumptions:

1. one provider final answer may be split across several ordered `output_text` items, so the
   semantic blocks are concatenated before strict JSON decoding;
2. a 2,048-token output limit repeatedly exhausted reasoning before final submission; one observed
   non-contiguous case required 2,346 output tokens and one evidence-aware Red review required
   7,854. The opt-in configs therefore permit 16,384 tokens per turn while retaining the 100,000
   cumulative provider-token ceiling per role.

The five-case Blue/Red matrix showed why a model verdict is evidence rather than authority. An early
Red schema could list false-accept/false-reject risks while still returning `pass` and “no revision”.
The current review separates typed blocking findings from advisories, and trusted validation
requires `pass` exactly when the blocker set is empty. Repeated reviews of `max-empty-axis` also
disagreed when search returned unrelated files. Changing the query to the upstream
`test_empty_tensor_empty_slice` identifier retrieved the relevant PyTorch reduction rule; Red now
receives the frozen draft's cited bounded evidence, but never Blue private history. Cross-run verdict
disagreement remains a forced-revision signal rather than a majority vote.

This still does not accept the complete Oracle Agent:

- move the dogfood-only typed draft/review bodies into the production Blue/Red submission gateways
  and materialize every referenced executable body, rather than asking the model to invent
  pre-existing content IDs;
- drive the loop through the generic durable `AgentEpisode` coordinator under the configured turn
  and cumulative-token budgets;
- execute complete Blue proposal submission, isolated Red attacks, trusted feedback/revision, and
  hardware-free admission with real model calls;
- add bounded retry/failure artifacts for turns that consume their output budget without a final
  structured submission;
- repeat the same sample/evidence boundary enough times to quantify draft/verdict stability and
  cache behavior without treating either cache hits or majority votes as correctness.

The full gate passes only after every rung above produces a reconstructable artifact graph and the
recorded counterpart remains deterministic in ordinary CI.
