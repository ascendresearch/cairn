# Resolved design decisions

- Status: normative decision register
- Date: 2026-08-24

This register records decisions that close entries in [`OPEN_QUESTIONS.md`](OPEN_QUESTIONS.md).
Detailed requirements remain in `SYSTEM_REQUIREMENTS.md`; detailed implementation boundaries remain
in `SYSTEM_DESIGN.md`.

## D-001 — Canonical JSON first, behind a codec boundary

- Resolves: OQ-001
- Decision: accepted

Cairn V1 will use canonical UTF-8 JSON for persisted structured artifacts, event payloads, fixtures,
and exported manifests.

The choice of JSON is an adapter decision, not a license for serialization details to spread through
domain and workflow code:

- domain types and state machines live in format-neutral crates;
- `cairn-codec` owns canonical JSON encoding/decoding, schema dispatch, and compatibility fixtures;
- persisted envelopes carry schema and encoding identifiers;
- content/event identity is computed from canonical encoded bytes under an explicit domain;
- a future encoding is introduced as a new codec/version and explicit transformation, never by
  silently changing the bytes an old identity means.

Canonical JSON V1 will define object-key ordering, UTF-8 handling, escaping, duplicate-key rejection,
number representation, and unknown-field behavior. Verdict-critical floating-point observations
will use exact integer bits, scaled integers, or specified strings instead of depending on a JSON
parser's floating-point round trip.

If “JSONP” means the browser callback format, it is not a persistence format: it may be a client-side
presentation wrapper someday, but it will not participate in canonical identity or durable storage.

## D-002 — SQLite first, behind storage ports

- Resolves: OQ-002
- Decision: accepted

Cairn V1 will use SQLite as the reference and production-first implementation for:

- append-only event streams and expected-revision checks;
- command idempotency and transactional outbox state;
- task/episode/operation projections;
- jobs, leases, attempts, and reconciliation state;
- artifact metadata and reference graph;
- subscription cursors and operational indexes.

Persistence is isolated behind narrow contracts:

- `EventStore` — append/read streams with expected revision and command idempotency;
- `ProjectionStore` — rebuildable views and checkpoints;
- `CoordinationStore` — leases, outbox, and process-manager checkpoints;
- `ContentStore` — immutable byte streaming and identity verification.

`cairn-store-sqlite` implements the first three. The initial `ContentStore` may use filesystem blobs
with SQLite metadata; callers do not depend on that layout. Product, agent, execution, verification,
and API crates depend on ports rather than `rusqlite` or SQL schemas.

SQLite-specific features such as WAL, busy timeout, transaction shape, and backup remain adapter
details. A future PostgreSQL or remote content-store adapter must pass the same contract and fault-
injection suites before it can replace SQLite.

## D-003 — Hybrid authority for the structured domain

- Resolves: OQ-003
- Decision: accepted

The supported domain is resolved from several separately recorded authorities rather than authored
entirely by one agent or required in full from one caller.

### Caller responsibility

The caller supplies a minimum machine-readable contract covering, for the requested claim:

- source entry point and parameter/buffer roles;
- dtypes and known shape/rank variables;
- declared valid ranges and required invalid/error behavior;
- requested output semantics or an independent semantic reference;
- explicit exclusions and unknowns.

The contract may be incomplete, but it may not disguise an unknown as an unrestricted domain.

### Cairn responsibility

A role-scoped domain-analysis/blue episode may propose refinements from source, documentation,
framework definitions, and upstream tests. Each refinement cites its evidence and remains distinct
from the caller declaration.

Cairn independently challenges both through:

- source implementation interrogation and boundary probing;
- mandatory cases derived from the caller's minimum contract;
- upstream and external test proposals with provenance;
- historical target-failure coverage obligations.

### Conflict rule

Caller declaration, agent interpretation, source observations, and external expectations are never
overwritten into one unattributed value. Admission records agreements and disagreements. An
unresolved disagreement affecting the requested claim rejects the oracle or reduces it to an
explicit weaker/`Unverifiable` result.

The admitted structured domain is an immutable artifact produced by admission. Changing it creates a
new oracle and experiment identity.

## D-004 — Policy-configured variant sufficiency

- Resolves: OQ-005
- Decision: accepted

There is no universal correct count of correct-by-construction and deliberately incorrect variants.
Variant sufficiency is defined by the immutable, versioned `AdmissionPolicy` selected for an
admission attempt.

The policy configures at least:

- minimum accepted correct-by-construction variants;
- minimum rejected implementation-level incorrect variants;
- required applicable fault/construction classes;
- structural-independence requirements;
- saturation rounds and what resets saturation;
- provider/execution budgets and the outcome when they are exhausted.

The V1 reference profile starts with two structurally distinct correct variants, three deliberately
incorrect variants, applicable fault-class coverage, and two consecutive saturation rounds. These
are profile defaults, not hard-coded verifier constants.

Counts are necessary controls, not sufficient evidence. Every accepted correct variant still needs
an independent `ConstructionClaim`; every mandatory incorrect variant must be rejected through the
required execution scope; and the generic mutant/case grid remains a separate obligation. A policy
that requests a stronger claim may increase these requirements. A profile may configure a smaller
family only if its accepted strength and limitations explicitly permit that reduction; it cannot
label missing evidence as having passed.

Budget exhaustion before the selected policy is satisfied yields rejection, a permitted weaker
strength, or `Unverifiable`. Cost limits never turn incomplete admission into `Pass`. The exact
policy identity and observed stopping reason are retained in the admission receipt.

## D-005 — Separate numerical provenance from assurance

- Resolves: OQ-008
- Decision: accepted

A numerical allowance records two orthogonal classifications:

1. **provenance** records where the allowance came from, such as `MeasuredFamily`,
   `MeasuredAdversarial`, `ExternalPrior`, `Asserted`, or `ExactOrSet`;
2. **assurance** records what the evidence justifies: `ProvenBound`, `ExhaustiveFinite`,
   `HeldOutValidated`, `ExploratoryMeasured`, `PriorOnly`, or `Unsupported`.

`HeldOutValidated` requires identity-disjoint derivation and validation corpora. The receipt records
variant count and independence, input-generation/search strategy, observed spans, seeds, domain
regions, and known unexplored regions. A safety factor is policy, not proof, and does not promote an
empirical maximum to `ProvenBound`.

`HeldOutValidated` may support an empirical `Pass` when allowed by the selected admission policy.
The verdict must label it as empirical and state its admitted domain, corpus relationship,
allowance, and limitations. Only a justified `ProvenBound` or `ExhaustiveFinite` assurance may use
an unqualified domain-wide numerical claim. `ExploratoryMeasured`, `PriorOnly`, and `Unsupported`
cannot be presented as such a claim.

Cairn does not report probabilistic confidence unless the input distribution and sampling procedure
are themselves declared and justified. A measured maximum alone is an observation, not a
statistical or mathematical bound.

## D-006 — MIT license for the public project

- Partially resolves: OQ-017 (outbound project license)
- Decision: accepted

Cairn source code and project-authored documentation will be released under the MIT License. Root
license text, Rust package metadata, generated release metadata, and source/fixture provenance must
agree with that choice.

This decision does not silently relicense imported code, corpora, model outputs, vendor SDKs, or
historical Cairn/Alloyport material. Such material requires compatible provenance or must remain an
external/private input. Contribution certification, governance, trademark policy, and the detailed
dependency-license gate remain to be decided before the first public release.

## D-007 — Typed SHA-256 identities with controlled migration

- Resolves: OQ-013
- Decision: accepted

Cairn V1 uses SHA-256 for content, event, and deterministic derived identities. Every public
identity carries an explicit frame version, algorithm tag, and semantic domain; Rust APIs retain the
domain as a concrete type such as `ContentId<SourceFile>` rather than an untyped digest.

The hash preimage is a versioned, length-delimited binary frame containing a fixed Cairn magic,
frame version, algorithm identifier, semantic domain, and exact canonical/payload bytes. Domains
are stable registered constants. Hashing uses a maintained cryptographic library and published test
vectors; Cairn does not implement SHA-256 itself.

Lifecycle identities such as task, episode, operation, job, attempt, command, and branch are
distinct UUIDv7-backed Rust types. UUID timestamp ordering is an operational convenience only and
never establishes event order, causality, or authority.

### Semantic and physical identities

- `ContentId<T>` is the public, domain-separated semantic identity for exact bytes under type `T`;
- `EventId` and `DerivedId<T>` are domain-separated identities for canonical relationship bytes;
- `BlobDigest` is an internal exact-byte storage/integrity digest used for physical lookup and
  deduplication.

The same bytes used under two semantic types therefore have different `ContentId<T>` values but may
share one `BlobDigest`. Product APIs never substitute `BlobDigest` for a semantic identity.

An `EventId` covers the canonical envelope without its own identity field, including stream,
sequence, schema, command causality, parent, observation time, and payload. Because sequence is
assigned under optimistic concurrency, trusted record code derives the event identity after
sequence allocation inside the append transaction; an untrusted caller does not supply it.

### Algorithm upgrade

V1 implements a closed SHA-256 algorithm enum rather than a speculative pluggable hashing
framework. An upgrade runs a controlled, restartable migration:

1. verify old semantic identities and physical bytes;
2. compute new blob and semantic identities;
3. write an immutable migration manifest containing the verified old-to-new mapping;
4. rebuild mutable metadata, projections, and indexes against the new identities;
5. atomically switch the active writer algorithm/version;
6. retain old events and exported identities as historical facts until their retention policy
   allows physical garbage collection.

Historical event bytes and verdict meaning are not rewritten. Legacy lookup consults the migration
manifest at import/resolution boundaries; ordinary new business logic does not carry a permanent
general alias graph. A failed migration resumes from verified checkpoints and cannot partially
authorize the new writer.

## D-008 — Semantic turns plus protocol-native continuation

- Resolves: OQ-012
- Decision: accepted

Cairn uses a hybrid record for model interaction. A provider-neutral semantic turn is the derived
contract consumed by the agent loop, tool validation, inspection, and semantic replay. Alongside it,
the selected protocol codec archives a lossless, versioned native continuation containing the
ordered protocol items and correlation identities required to materialize the next request.

There is deliberately no universal lossy `Message` type standing in for all protocols. OpenAI
Responses retains typed response items and function `call_id` relationships; OpenAI Chat
Completions retains the exact assistant message and `tool_call_id` relationships; Anthropic
Messages retains ordered content blocks and `tool_use_id` relationships. Reasoning, redacted, and
unknown policy-allowed native items are preserved without granting them semantic authority.

V1 reconstructs continuation locally from archived material. Responses uses full input with hosted
storage disabled; Chat Completions and Anthropic Messages rebuild their full native message/block
history. Provider response IDs and hosted continuation state may be recorded as external facts, but
are neither required for reconstruction nor treated as durable authority. A later hosted-state
optimization must retain a local fallback and prove the represented history boundary.

A catalog alias resolves a versioned model template, deployment, protocol, settings, transport
bounds, and a credential reference into a frozen secret-free snapshot. Protocol—not provider or
model name—selects the codec. Switching model, deployment, protocol, template revision, or codec
version creates a new episode or an explicit counterfactual branch.

## D-009 — Repository model templates and user deployment configuration

- Decision: accepted

Model characteristics are project-maintained versioned data under `model-templates/`, not fields an
operator must copy into runtime configuration. A template owns the provider-visible model name and a
separate section for every supported protocol. Each section declares context/output limits, tool,
parallel-call and reasoning capabilities, accepted reasoning efforts, schema dialect, safe defaults,
and protocol-specific request settings.

User runtime configuration owns only operational choices: enabled template alias, selected protocol,
endpoint, provider/account label, credential reference, data boundary, transport bounds, and optional
generation overrides. This permits a private deployment to reuse the same model template without
claiming that its endpoint defines the model's intrinsic capabilities. Overrides are validated
against the selected protocol section.

Resolution includes the exact typed template content identity and materialized characteristics in a
secret-free episode snapshot. A template update changes new episodes only. Endpoint conformance is a
separate measured fact and may later narrow or reject a configured deployment; it does not require
operators to maintain a duplicate capability profile.
