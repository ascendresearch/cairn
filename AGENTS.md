# Cairn agent rules

## Runtime-agent builder role

The coding agent working on this repository is the builder and outside observer of a generic
agent-based CUDA-to-Ascend-C migration application. DeepSeek (or another configured runtime model),
not the coding agent, is the per-task reasoning actor that reads previously unseen migration code,
uses authorized tools, and proposes intent, Oracle, or candidate artifacts. Therefore:

- Treat fixtures only as tests of the application and its authority boundaries. Never use a known
  fixture answer as product knowledge, runtime context, a hard-coded hypothesis, a deterministic
  recipe, or a production special case.
- Keep production APIs and prompts task-generic. They may describe how a runtime agent cites facts,
  represents competing hypotheses, requests evidence, or preserves unknowns; they must not encode
  `reduce-sum-f32`, D-039 identities, fixture-specific domains, or expected outcomes.
- Public fixtures may be visible to repository developers, but restricted expectations are visible
  only to the evaluation/admission side authorized for them. The runtime proposal agent must not
  receive hidden answers, receipt authority, or test-only identities.
- The coding agent may implement orchestration, typed boundaries, tools, persistence, evaluation,
  and mechanical policy checks. It must not substitute its own interpretation of a fixture for an
  actual runtime-model execution and then report the application as working.
- A first fixture is an integration control, not the architecture. Before generalizing a mechanism,
  show that a materially different migration task can run through the same production path without
  a product-code branch or fixture-derived prompt change.
- Treat SIR as a supported architectural extension point, neither a mandatory detour for every
  migration nor a disposable experiment. Preserve the smallest task-generic seam that has a real
  consumer, prioritize the end-to-end migration workflow, and expand SIR authority or topology only
  when a later consumer and a sufficiently stable architecture require it.
- Keep fixture-derived material out of production crates. A control that exists to exercise one known
  fixture belongs in test targets or test-only crates, not in a product crate's public API, even when
  it is labelled historical. If a product crate needs it, that is evidence the mechanism is not yet
  task-generic; fix the mechanism rather than exporting the fixture.

## Gates and ledgers

A gate that cannot be satisfied through an honest path does not stop anyone; it gets routed around.
These rules apply to the coding agent building Cairn, not only to the runtime model:

- Build the door you would have to walk through yourself, then walk through it. If the product needs
  to register knowledge, a skill, a policy or a control, it must use the same authorized path the
  runtime agent uses. A builder-only side entrance means the gate is decorative.
- Never hand-edit a ledger, a trust record, an audit file or any store the gates read. If the only way
  to record something is a text editor, the missing authorized path is the bug.
- Every gate must be runnable backwards over what is already inside, and the reading side and the
  auditing side must share one state-derivation function. A ledger with no auditor is not a gate.
- When a check is added, name the gate that consumes it. An alarm nothing is wired to is decoration.
- A verification that compares a thing to itself verifies nothing. This includes the case where the
  thing that failed to exist is the change: a transformation that rewrites nothing leaves the subject
  identical to the reference, and the comparison then passes tautologically.
- Evidence is what the harness recorded when a tool actually ran. A field an applicant fills in is a
  claim, never evidence, including when the applicant is this repository's own tooling.

## Knowledge and skill material

- Knowledge and skills ship as packs outside this repository. The repository carries fixtures for
  testing the application; it does not carry product knowledge.
- A pack ships content; the deployment owns trust. No pack may declare itself validated, and authorship
  is a provenance field, never a trust level.
- A fixture's algorithmic answer, expected output, task identity or bespoke prompt must never enter a
  pack, a production prompt or a runtime projection.
- Trust state binds to content identity. Changing the bytes of an entry retires every verdict bound to
  the previous bytes; do not carry a verdict across an edit.

## Development-stage versioning and compatibility

Until the user explicitly declares that Cairn has completed its first end-to-end workflow and is
establishing a public compatibility baseline, the repository is pre-release development state.
During this period:

- Do not add backward- or forward-compatibility logic, legacy aliases, fallback readers, dual
  readers/writers, import gates, data converters, or schema/protocol migration paths.
- Do not increment Cairn configuration, persisted-event, content-domain, wire-protocol, snapshot,
  artifact, or other internal format versions. The single current definition remains version 1.
- When a format or model changes, modify the V1 definition directly and update its code, tests,
  fixtures, examples, and documentation together. Development data and services may be discarded
  and rebuilt; no runtime conversion is required.
- Remove superseded development code and tests instead of retaining them solely to prove old-format
  compatibility. Tests should validate the current V1 contract and reject non-V1 input, not decode
  or transform it.
- If a requested change appears to require compatibility before that baseline exists, stop and
  surface the conflict instead of silently introducing versioning or migration machinery.

Product concepts whose names contain levels or vendor versions—such as migration validation tiers
V0–V3, model names, Rust/dependency versions, and Cargo package SemVer—are not Cairn internal format
versions and are outside this rule.

## Strong type system

Cairn uses the Rust type system as an authority boundary, not only as documentation.

- Semantically distinct identities, units, names, roles, schema versions, stream revisions,
  lifecycle states, evidence strengths, provenance classes, and policy outcomes must use distinct
  validated types. Do not erase them into interchangeable `String`, integer, digest, generic ID, or
  boolean values in production APIs.
- Raw wire/storage representations are allowed only at explicit codec, protocol, configuration, or
  persistence boundaries. Convert them immediately into the applicable strong type before domain
  logic sees them.
- Deserialization must re-run constructor invariants. A derived `Deserialize` implementation must
  not permit persisted or wire bytes to bypass validation performed by the public constructor.
- When two values are plausible to confuse, add a compile-fail or equivalent static boundary test
  showing that one cannot be passed where the other is required.
- Do not introduce a generic abstraction merely because two strong types currently share a
  representation. Share private validation or encoding mechanics while preserving their public
  semantic distinction.
