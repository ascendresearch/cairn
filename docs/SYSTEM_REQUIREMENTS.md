# Cairn system requirements

- Status: normative target baseline
- Date: 2026-08-24
- Product scope: unified Cairn
- Initial product slice: CUDA operator to Ascend C migration

## 1. Purpose

Cairn is an evidence-first agentic engineering system. Given a bounded heterogeneous-software
migration task, it searches for a candidate implementation, searches for and admits an oracle able to
judge that candidate, executes the relevant artifacts on controlled infrastructure, and returns a
verdict with an auditable evidence chain.

The generated implementation alone is not the product. A completed result consists of:

1. the implementation and its immutable identity;
2. the supported domain and target environment against which it was evaluated;
3. the oracle and the evidence that admitted that oracle;
4. source, build, execution, correctness, and optional performance receipts;
5. an explicit verdict strength, blind spots, assumptions, and unverified claims;
6. a durable execution record sufficient for audit, replay, and controlled counterfactual work.

The earlier Cairn harness and Alloyport product are one system here. Their separation survives only
as internal architecture and trust boundaries.

## 2. Product boundary

### 2.1 What Cairn owns

Cairn owns:

- task intake and immutable task identity;
- oracle proposal, attack, admission, versioning, and freezing;
- candidate search and correction loops;
- model, tool, context, and execution orchestration;
- local and remote execution on CPU, source accelerators, build environments, and target devices;
- evidence capture, content addressing, provenance, and verdict production;
- recovery, audit, deterministic recorded replay, and counterfactual continuation;
- a stable machine-facing API and a reference CLI;
- the open-source contracts required to add providers, domain adapters, and clients safely.

### 2.2 What remains outside Cairn

Cairn does not own:

- deciding which operator is globally worth migrating when that decision requires a model-level or
  fleet-level view;
- model-level acceptance thresholds such as whether an entire model is fast enough;
- cross-operator portfolio budgets held by an upstream caller;
- prescribing the search method from upstream feedback;
- proving facts that the available evidence cannot establish.

An upstream caller MAY provide measurements, constraints, priorities, and termination decisions. It
MUST NOT silently replace Cairn's search method or verification policy through an untyped instruction.

### 2.3 Initial and future scope

The initial end-to-end scope is a CUDA operator migrated to Ascend C. The architecture MUST admit a
second operator shape without modifying the agent runtime, execution substrate, worker protocol, or
generic verification kernel. Broader heterogeneous migrations MAY be added only after this property
is demonstrated, not inferred from generic names.

## 3. Actors and trust posture

| Actor or artifact | Role | Default trust |
|---|---|---|
| Upstream caller | supplies task, declared domain, constraints, and budget | authoritative only for its declared intent |
| Oracle author (blue) | proposes domain details, reference semantics, properties, and valid variants | untrusted proposal |
| Oracle breaker (red) | produces correct-by-construction and deliberately wrong variants | conditionally trusted per claim |
| Candidate author | searches for the target implementation | untrusted |
| Model provider | produces model responses | untrusted and nondeterministic external service |
| Source implementation | executable artifact being migrated | behavioral evidence, not infallible semantics |
| External corpus | upstream tests, OpInfo-like data, crawled material | proposal with provenance, never truth by origin |
| Verification kernel | derives tolerances, injects generic mutants, compares, adjudicates | trusted repository code |
| Execution worker | executes opaque authorized jobs and captures evidence | trusted only within its declared attestation boundary |
| Candidate sandbox | compiles or runs untrusted code | untrusted and isolated |
| Human reviewer/operator | authorizes cost or risk and inspects evidence | policy authority, not a substitute oracle |

Authorship MUST be recorded as provenance and MUST NOT by itself raise a claim's trust level. A
hand-written oracle and a model-authored oracle MUST face the same applicable admission requirements.

## 4. System outcomes

### 4.1 Result taxonomy

Cairn MUST distinguish the following terminal outcomes:

- `Pass`: the candidate satisfied an admitted oracle over the stated domain and evidence strength;
- `Fail`: the candidate contradicted an admitted check, with reproducible failure evidence;
- `Unverifiable`: the system could not establish a sufficiently strong oracle for the requested
  claim;
- `Incomplete`: authorized search ended without a candidate verdict;
- `Cancelled`: an authorized actor stopped the work;
- `BudgetExhausted`: the declared budget ended the work;
- `InfrastructureFailure`: the requested observation could not be obtained because Cairn or its
  environment failed.

`InfrastructureFailure` MUST NOT be converted into `Fail`, and `Unverifiable` MUST NOT be converted
into a weaker `Pass` without changing the requested claim and reporting the weaker strength.

### 4.2 Verdict contents

Every `Pass`, `Fail`, or `Unverifiable` result MUST name:

- task, candidate, oracle, domain, corpus, policy, and target identities;
- the oracle strength used: reference, property/metamorphic, implicit, or none;
- calibration/admission identity and scope;
- every failed case and applicable diagnostic;
- known blind spots and coverage exclusions;
- unverified assumptions such as device execution or runner attestation;
- the exact receipts and recorded events supporting the result.

## 5. Functional requirements

### 5.1 Task intake and identity

| ID | Requirement | Acceptance evidence |
|---|---|---|
| FR-TASK-001 | Cairn MUST accept a versioned task specification containing source artifacts, source entry point, target platform, declared supported domain, requested claims, budgets, and data/security policy. | Schema tests plus one real task loaded from serialized bytes. |
| FR-TASK-002 | Every immutable task input MUST be stored or explicitly classified as an external secret/reference before execution begins. | Backwards input audit returns no unexplained identities. |
| FR-TASK-003 | The task identity MUST be derived from canonical bytes and MUST change when any verdict-relevant input changes. | Mutation tests for every identity edge. |
| FR-TASK-004 | Derived identities MUST be distinguishable from content identities and MUST NOT be queried as though bytes necessarily exist for them. | Fake-store contract proves no derived identity is dereferenced. |
| FR-TASK-005 | The caller MUST supply a minimum machine-readable domain contract covering known buffer/parameter roles, dtypes/shapes, valid ranges, required error behavior, requested semantics, exclusions, and explicit unknowns. It MUST be sufficient to derive mandatory base boundary cases for the requested claim. | Domain-to-corpus tests with independently derived expected cases and an incomplete/unknown control. |
| FR-TASK-006 | Cairn MUST preserve the caller's original declaration and any later measured disagreement between declared and observed domain. | Evidence record showing both values without overwriting either. |
| FR-TASK-007 | Agent-proposed domain refinements MUST cite their evidence and remain distinct from caller declarations, source observations, and external expectations until admission adjudicates them. | Conflicting-domain fixture retains all sources and blocks an unattributed merged value. |

### 5.2 Oracle search and admission

| ID | Requirement | Acceptance evidence |
|---|---|---|
| FR-ORACLE-001 | Cairn MUST model an oracle as a versioned artifact that is proposed, admitted or rejected, and frozen before it judges a candidate. | State-machine tests and an end-to-end freeze control. |
| FR-ORACLE-002 | The candidate author MUST NOT be able to modify the oracle, corpus, tolerance policy, mutants, comparison, or adjudication for its own experiment. | Capability/visibility test plus sandbox write-isolation test. |
| FR-ORACLE-003 | Oracle proposal and kernel search MUST be separate episodes with separate visibility policies and linked immutable identities. | Record audit demonstrating the two episodes and link. |
| FR-ORACLE-004 | Cairn MUST support correct-by-construction variants that an oracle is required to accept. Their correctness argument MUST not depend on passing the oracle under test. | At least two structurally independent variants for two operator families. |
| FR-ORACLE-005 | Cairn MUST support deliberately incorrect variants that an oracle is required to reject. | Executed mutation controls shown red before restoration. |
| FR-ORACLE-006 | Generic mutants, tolerance derivation, comparison logic, and admission adjudication MUST be supplied by trusted repository code, not by an oracle or candidate proposal. | Dependency and artifact-origin checks. |
| FR-ORACLE-007 | Admission MUST exercise real implementation paths through build, execution, observation, and comparison; comparator-only mutation is insufficient for completion. | Admission receipt has implementation scope and cites executed variants. |
| FR-ORACLE-008 | Admission MUST distinguish false-accept detection, false-reject detection, announced-boundary enforcement, coverage, and honest-path acceptance as separate checks. | Receipt schema and negative controls for each check. |
| FR-ORACLE-009 | A tolerance MUST carry provenance. A tolerance derived only from assertion MUST NOT admit a numeric oracle requiring measured tolerance. | `Asserted` control returns `Unverifiable` or rejection. |
| FR-ORACLE-010 | A threshold derived from a measurement MUST NOT be validated solely by the same measurement. | Regression reproducing and preventing the historical self-validating threshold defect. |
| FR-ORACLE-011 | Mutation trials MUST cover every applicable mutant/case pair or explicitly explain why a pair is not injectable. An empty applicable grid MUST fail admission. | Grid completeness and empty-grid negative tests. |
| FR-ORACLE-012 | Policy-sized and scale-free misses MUST be fatal. Case-dependent misses MAY be non-fatal but MUST be recorded as blind spots with the swallowing boundary. | Per-case battery control. |
| FR-ORACLE-013 | Source implementation, high-precision reference, external cases, and properties MUST remain separate evidence sources so disagreements can be adjudicated rather than overwritten. | Three-way disagreement fixtures. |
| FR-ORACLE-014 | Cairn MUST support reference, property/metamorphic, implicit, and unavailable oracle strengths without representing them as equally strong. | Verdict serialization and UI/API conformance tests. |
| FR-ORACLE-015 | Updating an oracle, corpus, domain, comparison policy, or calibration evidence MUST create a new experiment identity; it MUST NOT mutate the meaning of an old verdict. | Immutable-history and versioning tests. |
| FR-ORACLE-016 | Oracle admission MUST produce an immutable admitted-domain artifact from the separately recorded caller contract, refinements, source observations, external proposals, and coverage obligations. An unresolved conflict affecting the requested claim MUST reject or weaken admission. | Agreement, resolved-conflict, and unresolved-conflict controls. |
| FR-ORACLE-017 | Variant counts, required construction/fault classes, independence, saturation, and exhaustion behavior MUST be versioned `AdmissionPolicy` configuration rather than hard-coded verifier constants. Failure to satisfy the selected policy MUST NOT produce `Pass`. | Multiple policy-profile tests, budget-exhaustion control, and recorded stopping reasons. |
| FR-ORACLE-018 | A numerical allowance MUST record provenance independently from assurance. Assurance MUST distinguish at least proven bounds, exhaustive finite coverage, held-out validation, exploratory measurement, external-prior-only, and unsupported evidence. | Serialization and adjudication fixtures for every class. |
| FR-ORACLE-019 | `HeldOutValidated` MAY support only an explicitly empirical `Pass`; an unqualified domain-wide numerical claim MUST require a justified proven bound or exhaustive finite coverage. Derivation and validation corpora MUST be identity-disjoint. | Corpus-overlap rejection, empirical-verdict labeling, and proven/exhaustive controls. |

Detailed proof obligations are normative in [`ORACLE_ADMISSION.md`](ORACLE_ADMISSION.md).

### 5.3 Candidate search and gates

| ID | Requirement | Acceptance evidence |
|---|---|---|
| FR-CAND-001 | Cairn MUST run bounded candidate-search episodes against a pinned model configuration, tool catalog, context policy, task, and admitted oracle. | Complete episode identity audit. |
| FR-CAND-002 | Candidate evaluation MUST separate at least source completeness, target build, target execution/correctness, and optional performance claims. | A fixture that fails independently at each stage. |
| FR-CAND-003 | A defect the model can inspect and correct MUST be represented as recoverable diagnostic feedback, not a fatal infrastructure error. | Wrong-citation, source, build, and correctness rejection recovery tests. |
| FR-CAND-004 | Ambiguous external effects and actual infrastructure defects MUST retain durable recovery semantics and MUST NOT be retried as though known not to have occurred. | Crash/restart fault-injection tests. |
| FR-CAND-005 | Gate inputs MUST be derived from trusted records or verified receipts where possible; the model MUST NOT retype values already carried by a cited artifact. | Tool schemas plus regression for wrong digest transcription. |
| FR-CAND-006 | A candidate rejection MUST include the minimal evidence needed to correct it without exposing secrets or allowing the candidate to change the gate. | Diagnostic contract tests and redaction tests. |
| FR-CAND-007 | The system MUST preserve every attempted candidate and its relationship to parent attempts rather than retaining only the final candidate. | Candidate lineage audit. |

### 5.4 Agent runtime

| ID | Requirement | Acceptance evidence |
|---|---|---|
| FR-AGENT-001 | The agent runtime MUST be domain-neutral: its production types and behavior MUST NOT depend on CUDA, Ascend, kernels, operators, or gates. | Compiler dependency boundary plus vocabulary gate. |
| FR-AGENT-002 | Model transport, semantic model adaptation, tool execution, and model-visible input selection MUST be explicit replaceable capabilities. | Recorded providers and scripted providers substitute without loop flags. |
| FR-AGENT-003 | Every decision affecting model-visible instructions, tools, history, injected context, model configuration, or pending tool results MUST be recorded before provider dispatch. | Runtime invariant and fault-injection test. |
| FR-AGENT-004 | The model request MUST be projected from durable facts and content, not from unrecorded mutable session state. | Restart-before-dispatch produces byte-identical request. |
| FR-AGENT-005 | Provider nondeterminism MUST be represented explicitly. Cairn MUST NOT claim that a live provider continuation is deterministic merely because its request was reconstructed. | Same-request live control records possible divergence. |
| FR-AGENT-006 | Agent roles MUST be isolated by scoped capabilities and visibility, not merely by prompt instruction. | Red/blue/candidate capability matrix test. |
| FR-AGENT-007 | The runtime MUST enforce explicit, independently configurable budgets for turns, tokens where observable, tool operations, wall time, and externally metered actions. Every dimension MUST support a typed configured value and an explicit disabled state. | Configuration round-trip plus enabled, disabled, boundary, and exhaustion tests. |
| FR-AGENT-008 | Cancellation and suspension MUST reach durable safe points and preserve whether each external operation is pending, completed, rejected, or ambiguous. | Cancellation at every operation phase. |
| FR-AGENT-009 | Runtime-model alias, provider-visible model, deployment, protocol, capability profile, generation policy, transport limits, and credential reference MUST be separate validated fields. Resolving an alias MUST produce an immutable, reconstructable, secret-free episode snapshot. | Catalog validation, resolution identity, reload, and secret-scan tests. |
| FR-AGENT-010 | Codec selection MUST depend on the configured protocol, never on provider or model-name branches. One wire model MAY be exposed through multiple deployments and protocols. | The same fixture model passes all configured protocol selections after its provider label is changed. |
| FR-AGENT-011 | The initial provider boundary MUST support OpenAI Responses, OpenAI Chat Completions, and Anthropic Messages as distinct protocol families behind the same domain-neutral turn contract. | Per-protocol golden request, response, tool-use, tool-result, reasoning, malformed-input, and continuation suites. |
| FR-AGENT-012 | Cairn MUST preserve protocol-native ordered response/continuation material needed for a later turn, including correlation identities and non-text blocks. A provider-neutral semantic turn MUST NOT be treated as a lossless replacement for native continuation. | Multi-turn tool and reasoning fixtures round-trip without dropping or inventing native blocks. |
| FR-AGENT-013 | Provider SDK/HTTP integration MUST perform exactly one bounded provider turn and MUST NOT execute client tools or own the agent loop. | Architecture boundary test and scripted transport conformance. |
| FR-AGENT-014 | Credentials MUST be resolved only at dispatch from an external typed reference. Credential bytes MUST NOT appear in model catalogs, resolved snapshots, request artifacts, events, logs, or exports. | Unknown-field rejection, dispatch injection, and repository/record secret scanning. |

### 5.5 Execution substrate

| ID | Requirement | Acceptance evidence |
|---|---|---|
| FR-EXEC-001 | Workers MUST execute opaque, versioned jobs and MUST NOT contain operator-specific mathematics or gate-specific adjudication. | Boundary test made red by an operator-specific fixture. |
| FR-EXEC-002 | Execution mechanism and product task kind MUST be separate types and protocol fields. | Protocol tests and no mixed enum. |
| FR-EXEC-003 | Every job MUST identify its immutable input bundle, environment/image, command contract, resource request, policy, and expected outputs. | Job identity mutation suite. |
| FR-EXEC-004 | Candidate-writable diagnostic files MUST NOT be used as trusted verdict inputs. Trusted worker evidence MUST be captured outside the candidate's write boundary. | Sandbox escape/control fixture. |
| FR-EXEC-005 | Workers MUST authenticate, advertise capabilities, obtain leases, renew them, and report attempts. The controller MUST reap expired leases and reconcile abandoned assignments. | Worker-loss and controller-restart integration tests. |
| FR-EXEC-006 | Duplicate live workers claiming the same stable identity MUST be rejected or assigned unique incarnations with an explicit diagnostic. | Duplicate-identity integration test. |
| FR-EXEC-007 | Retrying an execution MUST create a distinct attempt identity while preserving the logical job identity and prior evidence. | Retry lineage test. |
| FR-EXEC-008 | Resource selection, device visibility, container mapping, and executed binary/image identity MUST be recorded from the running environment rather than inferred from deployment paths. | Real or emulated deployment audit. |
| FR-EXEC-009 | Job stdout, stderr, exit status, timing, resource observations, and declared outputs MUST be captured without relying on a single streaming reader path. | Dual-path output capture and truncation controls. |

### 5.6 Cost and scheduling

| ID | Requirement | Acceptance evidence |
|---|---|---|
| FR-COST-001 | Validation MUST stop at the cheapest tier that can decide the current claim: CPU, source accelerator, target build, then target device. | Scheduler trace for failures at each tier. |
| FR-COST-002 | Model/provider spend MUST be budgeted separately from the validation ladder because model turns propose and repair artifacts rather than form a final linear tier. | Cost ledger distinguishes search spend from execution spend. |
| FR-COST-003 | Before requesting another paid correction turn, Cairn SHOULD collect all already-authorized cheaper diagnostics that can be safely obtained for the current proposal. | Workflow test showing aggregated diagnostics. |
| FR-COST-004 | Scarce target devices MUST NOT be consumed by a proposal that failed an applicable cheaper tier. | Scheduling invariant. |
| FR-COST-005 | Cost policy decisions MUST be recorded and replayable as decisions, including skipped tiers and their justification. | Record projection test. |

### 5.7 Records, audit, replay, and counterfactuals

| ID | Requirement | Acceptance evidence |
|---|---|---|
| FR-REC-001 | The durable record MUST be an append-only sequence of versioned facts. Mutable views and snapshots MUST be rebuildable projections, not independent truth. | Rebuild-from-zero equivalence test. |
| FR-REC-002 | Every model-visible byte MUST be reconstructable from the record and content store, or reported as a typed recording gap. | Backwards audit over every completed episode. |
| FR-REC-003 | Every verdict-relevant artifact and decision MUST be addressable by immutable identity and connected in a traversable evidence graph. | Verdict-to-source graph walk. |
| FR-REC-004 | Stored summary fields such as `passed` MUST NOT override derivable underlying trials or facts. Readers MUST recompute security- or verdict-relevant conclusions where feasible. | Tampered-summary mutation tests. |
| FR-REC-005 | Cairn MUST replay a recorded episode by substituting recorded model and tool providers without product-specific replay branches in the loop. | Full recorded replay with zero divergence. |
| FR-REC-006 | Cairn MUST distinguish byte replay, semantic replay, workflow replay, and live counterfactual continuation. | API/type tests prevent conflation. |
| FR-REC-007 | Counterfactual execution MUST identify its source record, cut boundary, changed variable, control, resulting branch, and first divergence. | One controlled same-input run and one deliberate perturbation. |
| FR-REC-008 | A live continuation from a recorded prefix MUST create a new branch and MUST NOT append to or rewrite the historical source run. | Branch immutability test. |
| FR-REC-009 | Secret material MUST NOT enter the content store or exported record; secret references and credential state MUST be represented without secret bytes. | Secret scanning and redaction tests. |
| FR-REC-010 | Content, event, and derived identities MUST use a versioned algorithm tag and registered semantic domain in both their hash preimage and wire representation. Distinct identity domains MUST be distinct Rust types. | Published SHA-256 test vectors, cross-domain inequality tests, parsing mismatch tests, and compile-fail controls. |
| FR-REC-011 | Event identity MUST be derived by trusted record code after aggregate sequence allocation and MUST cover the canonical envelope excluding only its own identity field. Callers MUST NOT author an `EventId` for append. | Envelope-field mutation suite and concurrent append control. |
| FR-REC-012 | Physical `BlobDigest` and semantic `ContentId<T>` MUST remain distinct. Identical bytes MAY share physical storage without making semantic identities interchangeable. | Same-bytes/different-domain fixture and deduplication contract test. |
| FR-REC-013 | An identity-algorithm upgrade MUST run as a restartable verified migration with an immutable mapping manifest, projection/index rebuild, and atomic writer cutover. It MUST NOT rewrite historical event bytes or verdict meaning. | Interrupted-migration recovery, old export resolution, and partial-cutover rejection tests. |

Detailed record semantics are normative in [`RECORD_REPLAY.md`](RECORD_REPLAY.md).

### 5.8 External interfaces and extensibility

| ID | Requirement | Acceptance evidence |
|---|---|---|
| FR-API-001 | Cairn MUST expose a versioned bidirectional API for task lifecycle, event streaming, approvals, artifact access, worker control, and verdict retrieval. | Generated schema plus compatibility suite. |
| FR-API-002 | External lifecycle resources MUST reflect product concepts such as task, episode, attempt, artifact, oracle, and verdict; internal low-level events MUST be translated rather than leaked wholesale. | Client contract tests. |
| FR-API-003 | The API MUST provide stable item lifecycles for streaming work: started, zero or more updates, and terminal completion/failure. | Ordering and reconnect tests. |
| FR-API-004 | Clients MUST be able to reconnect and reconstruct a consistent timeline from durable facts without depending on missed ephemeral notifications. | Disconnect/reconnect integration test. |
| FR-API-005 | In-process clients and out-of-process transports SHOULD share generated protocol types and lifecycle behavior. | Reference CLI and test client conformance. |
| FR-EXT-001 | In-tree capabilities SHOULD use Rust traits and typed registries. Out-of-tree executable capabilities MUST use a versioned process protocol. Cairn MUST NOT depend on an unstable native dynamic Rust ABI. | Extension examples and ABI boundary review. |
| FR-EXT-002 | A capability extension MUST declare its service contract, provider, consumer, durable events, permissions, and failure semantics. | Extension manifest/schema validation. |

## 6. Quality requirements

### 6.1 Auditability and correctness

| ID | Requirement | Acceptance evidence |
|---|---|---|
| QR-AUD-001 | No `Pass` may depend solely on an applicant's self-reported conclusion. | Adversarial receipt fixtures. |
| QR-AUD-002 | Every new gate or invariant MUST be demonstrated red under a verified perturbation before being accepted green. | Mutation log in test output/evidence. |
| QR-AUD-003 | Every batch check MUST report both what it caught and what it did not exercise. | Coverage/blind-spot receipt fields. |
| QR-AUD-004 | A reader that can succeed while returning no required data MUST be treated as failure or explicitly empty by contract. | Empty-read negative tests. |

### 6.2 Reliability and recovery

| ID | Requirement | Acceptance evidence |
|---|---|---|
| QR-REL-001 | Controller restart MUST not lose committed events, completed artifacts, operation authority, leases, or the ability to reconcile work. | Kill/restart test matrix. |
| QR-REL-002 | A crash between external effect and acknowledgement MUST become `Ambiguous` unless an idempotency mechanism proves the result. | Fault injection at commit boundaries. |
| QR-REL-003 | The system MUST detect corrupt, missing, or identity-mismatched content before using it in a model request or verdict. | CAS corruption suite. |
| QR-REL-004 | Event consumers MUST tolerate replay and duplicate delivery using stable event and operation identities. | At-least-once delivery tests. |

### 6.3 Security and isolation

| ID | Requirement | Acceptance evidence |
|---|---|---|
| QR-SEC-001 | Untrusted candidate and oracle code MUST execute in a sandbox with least privilege, explicit mounts, bounded resources, and denied network access by default. | Sandbox policy tests. |
| QR-SEC-002 | Tool and execution authorization MUST be enforced by code outside the model-visible instruction channel. | Prompt-injection resistance control. |
| QR-SEC-003 | Shared-device scheduling MUST prevent accidental use of unauthorized or quarantined devices. | Device policy integration tests. |
| QR-SEC-004 | Logs and exported evidence MUST redact credentials and policy-defined sensitive source content. | Redaction fixtures and secret scanner. |
| QR-SEC-005 | Vendored corpora, skills, images, and fixtures MUST carry source and license provenance before release. | Release compliance gate. |

### 6.4 Maintainability and compatibility

| ID | Requirement | Acceptance evidence |
|---|---|---|
| QR-MNT-001 | The project MUST be a single Rust workspace with enforced dependency direction and no sibling-repository path dependency. | Workspace and boundary checks. |
| QR-MNT-002 | The workspace MUST define and continuously test an MSRV, formatting, lint, unit, integration, boundary, schema, and mutation baseline. | One authoritative verification command. |
| QR-MNT-003 | Persisted and wire schemas MUST be versioned and have explicit compatibility/migration policy. | Old-fixture read tests and protocol compatibility tests. |
| QR-MNT-004 | A second real operator MUST not require changes to generic runtime, worker, or verification core types. | Second-operator end-to-end control. |
| QR-MNT-005 | Architecture documents MUST state whether a described capability is target, implemented, or measured. | Documentation gate/review checklist. |
| QR-MNT-006 | Semantically distinct identities, revisions, schema versions, evidence strengths, and lifecycle states MUST use distinct validated Rust types. Production APIs MUST NOT erase them into interchangeable strings, integers, digests, or generic identifiers except at explicit wire/storage boundaries. | Compile-fail boundary tests plus schema round-trip and invalid-value tests. |

### 6.5 Observability and cost accountability

| ID | Requirement | Acceptance evidence |
|---|---|---|
| QR-OBS-001 | Every task, episode, step, operation, job, attempt, artifact, oracle, calibration, candidate, and verdict MUST have a stable correlation identity. | Trace-to-record reconciliation test. |
| QR-OBS-002 | Metrics and logs MUST be derivable from or correlated to durable events without becoming verdict authority. | Observability projection test. |
| QR-OBS-003 | Provider tokens/cost when supplied, wall time, device time, build time, and artifact volume MUST be attributable to a task and attempt. | Cost report fixture. |

### 6.6 Open-source delivery

| ID | Requirement | Acceptance evidence |
|---|---|---|
| QR-OSS-001 | Cairn project-authored source code and documentation MUST be releasable under the MIT License. Package metadata, release artifacts, and root license text MUST agree; imported materials MUST retain compatible, explicit provenance. | Root license, package metadata, release-manifest, and provenance checks. |
| QR-OSS-002 | The public repository MUST include contribution, security-reporting, governance, compatibility, and release documentation before its first public release. | Release checklist. |
| QR-OSS-003 | A contributor without private hardware MUST be able to run the core, record, oracle-comparator, protocol, and replay suites using fixtures or emulators. | Hardware-free CI lane. |
| QR-OSS-004 | Hardware-specific tests MUST declare requirements and fail as skipped/unavailable rather than silently pass without executing the target path. | CI capability tests. |

## 7. Acceptance milestones

These milestones order evidence; they are not a promise of dates.

### M0 — Normative baseline

- requirements, system design, oracle admission, and record/replay documents reviewed together;
- unresolved choices live in `OPEN_QUESTIONS.md`;
- no implementation claim is made.

### M1 — Record kernel

- versioned event envelope and content store;
- model-input projection and complete-input audit;
- recorded model/tool providers;
- old complete-episode fixture replays with no unexplained gap;
- same-input live continuation is correctly described as a counterfactual control, not deterministic
  replay.

### M2 — Executed oracle admission

- structured domain and corpus derivation;
- hand-written reduction oracle represented as an ordinary proposal;
- correct and incorrect variants compile and execute;
- `battery_scope` covers implementation execution rather than only comparator receipts;
- a known historical false-reject fixture passes after measured family spread;
- at least one known blind spot is retained explicitly.

### M3 — First unified migration

- one CUDA-to-Ascend task runs through oracle admission and kernel search;
- a gate rejection is corrected without terminating the task;
- worker loss and controller restart are survived;
- final verdict graph is complete and exportable.

### M4 — Generalization control

- a second operator with a materially different output or call shape runs end to end;
- no generic runtime, worker, or verification-core type changes are needed;
- required domain-specific behavior enters as content-addressed artifacts or product adapters.

### M5 — Public platform surface

- versioned App Server API and reference CLI;
- documented provider and domain extension examples;
- hardware-free CI, security policy, contribution guide, licenses, and reproducible release artifacts.

## 8. Non-goals for the initial release

- feature parity with general-purpose coding agents;
- a dynamic in-process plugin marketplace;
- a general durable-workflow engine unrelated to agent and migration semantics;
- multi-agent teams before red/blue/candidate role isolation and record branching are proven;
- automatic trust of LLM-generated tests, crawled corpora, or upstream labels;
- silent context compaction before compacted inputs can be recorded and audited;
- model-level performance acceptance or global operator selection;
- claiming target-device execution or attestation that the deployment cannot independently observe.

## 9. Requirement-change rule

A requirement may be changed when evidence shows it is wrong, insufficient, or unaffordable. The
change MUST preserve the historical verdict's original meaning, identify affected acceptance tests,
and say whether existing evidence is invalidated, superseded, or merely interpreted more precisely.
