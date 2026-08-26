# Oracle Agent design

- Status: normative focused design and active implementation contract
- Date: 2026-08-26
- Parent design: [`SYSTEM_DESIGN.md`](SYSTEM_DESIGN.md)
- Verification design: [`ORACLE_ADMISSION.md`](ORACLE_ADMISSION.md)
- Decisions: `D-003`, `D-004`, `D-008`, `D-020`, `D-021`
- Requirements: `FR-TASK-005..007`, `FR-ORACLE-*`, `FR-AGENT-*`, `FR-COST-*`

## 1. Purpose

The Oracle Agent turns a caller's minimum structured operator contract and immutable source inputs
into an oracle proposal that trusted admission code can accept, reject, or classify as
unverifiable. It is the first search stage of a Cairn migration. It runs before candidate search and
does not require a target device merely to author or revise a proposal.

The Oracle Agent is not the adjudicator. Models may propose domain refinements, references,
properties, corpus cases, correct-by-construction variants, deliberately wrong variants, and
research leads. Trusted repository code derives mandatory cases and generic mutants, executes the
selected admission policy, derives numerical allowances, compares observations, and emits the
admission outcome.

The design has four simultaneous goals:

1. keep caller declarations, model interpretations, source observations, and external expectations
   separate until admission;
2. give blue and red opposed, server-enforced roles without sharing private conversation history;
3. make external research useful without treating an upstream project as automatic truth;
4. preserve high prompt-cache reuse without weakening role isolation or durable reconstruction.

## 2. Product position

```text
Created
  -> InputsResolved
  -> OracleSearch
       -> BlueEpisode
       -> RedEpisode
       -> ExecutedAdmission
       -> proposal revision, Rejected, Unverifiable, or AdmittedOracle
  -> OracleAdmitted
  -> CandidateSearch
  -> VerdictReady
  -> Completed
```

Oracle search belongs to M2. Candidate search and target-device judgment belong to M3. A scarce or
occupied NPU may block later V3 evidence, but it does not block structured intake, model-authored
proposal work, external research, CPU/source admission, or creation of an oracle whose target
assumptions and revalidation triggers remain explicit.

## 3. Input authority

### 3.1 Caller minimum contract

The caller supplies a canonical `MigrationDomainContractV1` sufficient to identify:

- the source entry point and ABI-ordered buffer/scalar roles;
- dtypes, fixed or symbolic shapes, rank, and known ranges;
- known valid inputs and required invalid/error behavior;
- requested output semantics or an independently supplied semantic reference;
- exclusions and explicit unknowns;
- source artifacts and the requested target platform;
- task, provider, tool, network, data-boundary, and spending policy.

The caller is not required to enumerate the full admitted domain or complete test corpus. Missing
knowledge must be an explicit unknown rather than an unrestricted claim.

### 3.2 Blue responsibility

Blue receives the exact caller contract, source artifacts, admitted documentation inputs, mandatory
base obligations derived by trusted code, and a scoped tool catalog. Blue may propose:

- evidence-citing domain refinements;
- semantic or higher-precision references;
- properties and metamorphic relations;
- valid-family construction strategies;
- additional corpus cases and adversarial regions;
- correct-by-construction implementations and construction claims;
- source-interrogation and observation plans;
- external research queries and imported test proposals.

Blue never overwrites the caller contract. Each refinement is an immutable delta citing its exact
evidence. Blue cannot choose the admission policy, derive a trusted tolerance, inject generic
mutants, compare its own evidence authoritatively, or emit an admitted oracle.

### 3.3 Red responsibility

Red receives the caller contract and the frozen public contract of a specific blue proposal, not
blue's unsubmitted reasoning or mutable work state. Red may propose:

- structurally independent correct variants intended to expose false rejects;
- deliberately incorrect variants intended to expose false accepts;
- adversarial inputs, boundary challenges, and disagreement evidence;
- fault-injection and construction claims required by the selected policy.

Red cannot mutate the blue proposal, admission policy, comparator, or final decision. Policy may
allow red its own external research capability later; the first V1 product profile requires the
external-test search tool for blue and keeps every result visible to red only through a frozen,
content-addressed proposal edge.

### 3.4 Trusted admission responsibility

Trusted code independently derives mandatory cases, selects the configured mutant set, schedules
authorized execution, verifies authoritative receipts, derives allowance provenance and assurance,
compares complete observations, and emits `Rejected`, `Unverifiable`, or `AdmittedOracle`.
Origin—caller, PyTorch, another framework, model, or repository—does not grant truth.

## 4. Episode and information model

One oracle-search attempt has at least two distinct Cairn episodes:

| Episode | Stable role | Private history | Required write capability |
|---|---|---|---|
| blue | oracle author/domain analyst | blue turns and blue tool results | refinements, proposal material, external research request |
| red | oracle breaker | red turns and red tool results | correct/wrong variants and attacks |

The episodes have distinct `EpisodeId` values, event streams, model snapshots, budgets, and
server-enforced capability sets. They share immutable task/source/context artifacts by typed content
identity. Cross-role communication uses submitted artifacts and trusted diagnostics; private native
continuation and reasoning blocks never cross the boundary automatically.

Blue and red are role scopes, not necessarily different model vendors. A policy may require
different model families or additional child episodes to support an independence claim. Such child
episodes return content-addressed reports and do not silently inherit a parent's private history.

Provider-hosted conversation state is optional optimization state, never reconstruction authority.
V1 continues to reconstruct every request locally from durable continuations.

## 5. Cache-aware model input

### 5.1 Correct optimization target

The product optimizes total uncached input tokens, total provider cost, latency, and admission
quality—not cache-hit percentage alone. Combining blue and red into one conversation could produce
a superficially high hit percentage while sending a larger mixed history, leaking role-private
context, and preventing independent budgets and replay.

### 5.2 Prefix layout

Each role projects model input in this order where the selected protocol permits it:

```text
stable protocol/tool prefix
stable Cairn oracle-search rules
stable role rules
stable caller contract and source snapshot
stable admission-policy public contract
append-only submitted artifacts and observations
current diagnostics and current request
```

Within an episode, existing blocks are never regenerated with timestamps, queue positions, mutable
status prose, or reordered tools. New evidence is appended. Caller declarations remain unchanged;
refinements are new artifacts. This simultaneously preserves D-003 provenance and provider prefix
reuse.

Tool definitions use deterministic ordering. A role is not offered a capability merely to improve
cache reuse. A common safe read-only prefix may be shared; role-specific tools remain governed by
the exact tool catalog and server registration.

### 5.3 Cache policy and evidence

Cache request controls are protocol/template behavior and remain in the resolved model snapshot.
They do not alter semantic continuation or role authority. Provider-returned cache read, write, and
miss token counts are optional metering observations attached to the same response receipt as total
input/output usage. Missing cache detail means cache efficiency is unknown, not zero.

Cache reuse is never evidence that two requests or outputs are semantically identical. Replay and
verdict logic ignore it. Reports may derive a hit ratio only when the provider supplied compatible
counts and must retain protocol/provider attribution.

## 6. External test research

### 6.1 Blue tool contract

The first tool is `oracle_search_external_tests`. It is registered as `ReadOnly` and receives a
strict canonical request containing:

- a bounded textual query;
- one or more operator-approved repository scopes;
- a positive bounded result count;
- an explicit source kind, initially GitHub-hosted source;
- optional path/language constraints represented by the provider-specific request adapter.

The model cannot provide arbitrary headers, credentials, redirect policy, filesystem paths, or an
unrestricted fetch URL. The trusted provider adapter owns endpoint construction and repository
allowlists. Credentials are resolved at invocation from external references and never enter tool
arguments, events, CAS, or model-visible results.

The result contains, for each fetched test proposal:

- repository identity and file path;
- immutable upstream blob/revision identity where the source supports it;
- canonical source URL and exact fetched-byte identity;
- a deterministic line-addressed excerpt capped per result for model context, while the full exact
  blob is archived outside the prompt;
- retrieval observation time and provider attribution;
- truncation and omitted-result facts;
- the exact query and scope that produced it.

Search-result snippets and fetched bytes cannot become executable corpus cases. They are
model-visible research context. Blue must independently express the learned boundary, invariant,
or failure mode in a Cairn-authored structured test proposal and cite the research-result identity
that informed it. The research archive deliberately has no `CorpusCaseArtifact` edge.

Red does not receive Blue's private continuation or unsubmitted reasoning. It may receive the
frozen proposal's cited bounded research context because that evidence is part of the public
proposal boundary. Red findings distinguish admission-blocking defects from optional advisories;
trusted validation requires `pass` exactly when the blocking set is empty. Repeated reviews that
disagree on the verdict are instability evidence and force revision or further evidence rather than
majority-vote admission.

### 6.2 Research-to-proposal boundary

PyTorch and other upstream tests are research inputs. They may reveal shape boundaries, dtype
behavior, error behavior, numerical expectations, and historical bugs, but admission must reconcile
them with caller intent, source observations, and independent semantics.

Cairn does not perform repository-license lookup in this research loop because the retrieved source
is not imported as a fixture or implementation. Repository/path/blob/byte provenance is retained so
we can reconstruct what Blue saw. If any future workflow actually vendors or distributes upstream
material, the ordinary imported-material release controls apply outside this loop. Repository name,
popularity, or retrieval success never promotes a case to trusted status.

### 6.3 Network and security boundary

The network tool is separately configurable from model-provider access. Policy fixes allowed
providers, repositories/hosts, query/result/response bounds, credential reference, data boundary,
and external meter. Redirects across authorities, private/link-local addresses, unbounded bodies,
active content, and model-selected authentication are rejected before fetch.

Search and fetch are read-only external effects and may be retried only under the normal durable
operation rules. Every live adapter has a recorded provider substitute so hardware-free/offline CI
replays exact results without network access.

## 7. Proposal and feedback loop

```text
blue submits proposal Vn
  -> red attacks frozen Vn
  -> trusted code executes all currently authorized cheap admission work
  -> one typed diagnostic bundle is assembled
       agreements
       caller/source/external disagreements
       missing policy obligations
       false accepts / false rejects
       blind spots
       infrastructure-unavailable evidence
  -> recoverable subject defect: feedback to the appropriate existing role episode
  -> changed proposal material creates Vn+1
  -> admission is rerun under its frozen policy
```

A provider correction turn is requested only after available cheaper diagnostics are collected.
Proposal revisions are immutable; the attempt graph retains parents and reasons. Changing model,
deployment, protocol, template revision, codec, or role scope starts a new episode or explicit
counterfactual branch, not an in-place mutation.

Admission may complete without NPU evidence when the selected strength permits it. The resulting
oracle must carry target/device assumptions, unverified claims, blind spots, and exact revalidation
triggers. It cannot claim target-specific coverage that was not executed.

## 8. Strong types at necessary boundaries

The product layer uses closed types for `Blue` versus `Red`, external-source kind, search
request/result identity, proposal revision, admission outcome, and cache token
observations. It reuses generic `EpisodeId`, `OperationId`, `ContentId<T>`, token quantities, and
tool-effect types rather than wrapping every internal string or integer.

The generic `cairn-agent` crate remains unaware of CUDA, Ascend, oracle gates, and PyTorch. It owns
episode mechanics, model protocols, tool execution, usage receipts, and reconstruction. The
`cairn-migration` product layer owns oracle roles, tool schemas, external-test provenance, prompt
projection, and the process manager that connects role outputs to verification.

## 9. Failure semantics

| Failure | Classification | Consequence |
|---|---|---|
| malformed model search arguments | subject/tool rejection | return typed correction diagnostic |
| disallowed repository or host | policy rejection | no network authority |
| definite HTTP rejection | read-only attempt rejected | diagnostic; policy may authorize retry |
| timeout after request may have completed | read-only ambiguous attempt | retry only through durable operation policy |
| conflicting upstream and source behavior | admission disagreement | weaken, reject, or request evidence |
| cache detail absent | observability gap | no fabricated zero/hit ratio |
| NPU unavailable | infrastructure block for target evidence | continue device-free oracle work; retain trigger |

## 10. Acceptance controls

The Oracle Agent slice is accepted only when tests demonstrate:

1. an incomplete-but-honest caller contract retains explicit unknowns;
2. blue and red use distinct episodes and cannot exchange private histories;
3. blue is offered the external-test tool and red is denied it by the first role policy;
4. a recorded PyTorch-like search returns exact source and retrieval-provenance facts;
5. changed query, repository, path, blob identity, or bytes changes evidence identity;
6. external research bytes have no typed promotion path to an executable corpus case;
7. a separately authored Blue proposal may cite research-result identity without copying the
   research artifact into the corpus;
8. two turns reconstruct byte-identical stable prefixes after restart;
9. provider cache usage is recorded when present and remains explicitly unknown when absent;
10. admission rejection returns typed diagnostics to the correct role and creates a new immutable
    proposal revision;
11. the hardware-free historical reduction control can be driven through the model-authored
    proposal boundary before its oracle judges a candidate;
12. no target-device claim is promoted while NPU evidence is unavailable.

## 11. Deferred work

- general-purpose web crawling or browser automation;
- arbitrary model-selected URLs or network credentials;
- importing or distributing upstream source through the research tool;
- cross-provider cache equivalence claims;
- role merging for cost reasons;
- target-device execution, which remains a later validation tier;
- multiple independent red teams until the selected admission policy requires them.
