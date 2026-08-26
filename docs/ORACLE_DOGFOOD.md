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

After installing the GitHub token, replace the last argument with the live-GitHub example. The gate
prints typed identities, usage/cache counts, and closure booleans only. It does not print secrets,
model reasoning, answer text, or fetched source.

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

This is only the first dogfood rung. It does not yet accept the complete Oracle Agent:

- run the same Blue research turn against live GitHub;
- materialize independently authored model drafts and their referenced bodies, rather than asking
  the model to invent pre-existing content IDs;
- drive the loop through the generic durable `AgentEpisode` coordinator under the configured turn
  and cumulative-token budgets;
- execute complete Blue proposal submission, isolated Red attacks, trusted feedback/revision, and
  hardware-free admission with real model calls;
- repeat enough turns to evaluate cache behavior without treating cache hits as correctness.

The full gate passes only after every rung above produces a reconstructable artifact graph and the
recorded counterpart remains deterministic in ordinary CI.
