# Cairn design baseline

- Status: normative target design
- Date: 2026-08-29
- Scope: CUDA → Ascend C migration system

This directory is the authoritative design baseline for the rewrite. It replaces neither the old
repositories' evidence nor their histories. It separates current decisions from historical
narrative so that a future implementation cannot be mistaken for complete merely because a design
document is complete.

## Current design overlay

Two later current-V1 documents now lead the reading order:

- [`design/CAIRN_CURRENT_PRODUCT_DESIGN.md`](design/CAIRN_CURRENT_PRODUCT_DESIGN.md) defines the
  current product mission, four planes and D/E experiment direction;
- [`design/SIR_ORACLE_CURRENT_DESIGN.md`](design/SIR_ORACLE_CURRENT_DESIGN.md) is the focused
  authority for source understanding, optional focused SIR, evolving/qualification Oracle,
  exploratory Candidate interaction and Candidate promotion.

Where older workflow or Oracle documents require a mandatory SIR stage or a complete Oracle before
any exploratory Candidate, the current focused document directly updates that current-V1 timing.
The older requirements and designs continue to govern obligations that the current documents do not
explicitly change; they remain valuable historical rationale, not an implicit compatibility path.

## Document map

| Document | Question it answers | Authority |
|---|---|---|
| [`design/CAIRN_CURRENT_PRODUCT_DESIGN.md`](design/CAIRN_CURRENT_PRODUCT_DESIGN.md) | What is the current product mission and experiment direction? | Current global product design |
| [`design/SIR_ORACLE_CURRENT_DESIGN.md`](design/SIR_ORACLE_CURRENT_DESIGN.md) | How do focused SIR, assurance, Oracle qualification and Candidate promotion currently compose? | Current focused SIR/Oracle authority |
| [`SYSTEM_REQUIREMENTS.md`](SYSTEM_REQUIREMENTS.md) | What must the system do, refuse, expose, and prove? | Normative requirements |
| [`SYSTEM_DESIGN.md`](SYSTEM_DESIGN.md) | What architecture is intended to satisfy those requirements? | Normative target design |
| [`design/README.md`](design/README.md) | How are the target code, logical, and runtime architectures organized? | Normative software architecture index |
| [`design/WORKFLOW_ARCHITECTURE.md`](design/WORKFLOW_ARCHITECTURE.md) | How do the Controller state machine, role-scoped Agent Loops, unified Workers, feedback routes, and direct network compose? | Normative workflow architecture |
| [`design/CODE_ORGANIZATION.md`](design/CODE_ORGANIZATION.md) | Which crates/modules own each rule and which dependency directions are forbidden? | Normative code organization |
| [`design/LOGICAL_ARCHITECTURE.md`](design/LOGICAL_ARCHITECTURE.md) | How do aggregates, commands/events, ports, capabilities, and workflows compose? | Normative logical architecture |
| [`design/RUNTIME_ARCHITECTURE.md`](design/RUNTIME_ARCHITECTURE.md) | Which processes, stores, trust zones, data paths, and recovery rules exist at runtime? | Normative runtime architecture |
| [`design/ADMISSION_ARCHITECTURE.md`](design/ADMISSION_ARCHITECTURE.md) | How do kind-specific Planner profiles, required evidence, execution, and mechanical Gates compose? | Normative Admission software architecture |
| [`design/AGENT_ARCHITECTURE.md`](design/AGENT_ARCHITECTURE.md) | Which Agent-capable functions and profiles exist, and how do episodes, Hosts, processes, and artifact-mediated interaction compose? | Normative Agent software architecture |
| [`dev/README.md`](dev/README.md) | How is development staged, sliced, gated, parallelized, and reconciled with the current implementation baseline? | Normative development-planning index; no implementation authorization |
| [`dev/NEXT_SESSION.md`](dev/NEXT_SESSION.md) | Where should the next session start, what must it verify, and what is the proposed next slice? | Current session handoff |
| [`oracle/README.md`](oracle/README.md) | In what order should the Oracle research and subsystem designs be read? | Oracle document index |
| [`oracle/DESIGN_INVARIANTS.md`](oracle/DESIGN_INVARIANTS.md) | Which cross-document invariants and pre-implementation checks must every later session preserve? | Normative cross-cutting guardrail |
| [`oracle/SEMANTIC_INTENT_RECOVERY_DESIGN.md`](oracle/SEMANTIC_INTENT_RECOVERY_DESIGN.md) | How are high-order user-intent hypotheses recovered without giving the extractor contract authority? | Normative subsystem design |
| [`oracle/ORACLE_EXPLORATION_SYSTEM_DESIGN.md`](oracle/ORACLE_EXPLORATION_SYSTEM_DESIGN.md) | How are multi-plane Oracle claims explored, attacked, and prepared for independent admission? | Normative subsystem design |
| [`oracle/INDEPENDENT_ADMISSION_DESIGN.md`](oracle/INDEPENDENT_ADMISSION_DESIGN.md) | How do planner agents, hidden controls, authoritative receipts, and mechanical gates divide responsibility across all admission kinds? | Normative cross-cutting design |
| [`oracle/PERFORMANCE_ORACLE_DESIGN.md`](oracle/PERFORMANCE_ORACLE_DESIGN.md) | How are hardware facts, microbenchmarks, profiling, conditional rooflines, and performance claims admitted? | Normative subsystem design |
| [`oracle/KNOWLEDGE_AND_SKILL_TRUST_DESIGN.md`](oracle/KNOWLEDGE_AND_SKILL_TRUST_DESIGN.md) | How may agents retrieve knowledge and load skills without turning origin or retrieval into trust? | Normative subsystem design |
| [`oracle/ORACLE_ADMISSION.md`](oracle/ORACLE_ADMISSION.md) | Why may an Oracle claim judge a candidate, and how do previous-round feedback and all validation planes enter admission? | Normative verification design |
| [`OBSERVABILITY.md`](OBSERVABILITY.md) | Which runtime events are logged, how are they correlated, and what data is forbidden? | Normative operational design and coverage audit |
| [`RECORD_REPLAY.md`](RECORD_REPLAY.md) | What is recorded, reconstructed, replayed, and compared? | Normative record design |
| [`DECISIONS.md`](DECISIONS.md) | Which formerly open choices have been resolved? | Normative decision register |
| [`OPEN_QUESTIONS.md`](OPEN_QUESTIONS.md) | What has not been decided? | Explicitly non-normative decision backlog |
| [`RELEASE.md`](RELEASE.md) | How are x86-64/AArch64 artifacts cross-linked, inspected, and promoted? | Normative release procedure |
| [`ENROLLMENT.md`](ENROLLMENT.md) | How does a worker bootstrap a local managed identity without copied private keys? | Operator procedure and implemented trust boundary |
| [`SCHEDULER.md`](SCHEDULER.md) | How are generic candidates selected, reserved, assigned, retried, and safely released? | Implemented scheduling trust boundary |
| [`RESOURCE_PROBING.md`](RESOURCE_PROBING.md) | Which resource facts are observed, configured, matched, and still deferred? | Implemented D1 probe/operator contract |
| [`WORKER_EXECUTION.md`](WORKER_EXECUTION.md) | How is Docker activated, recovered, and measured? | Implemented F2 operator boundary |
| [`oracle/ORACLE_RESEARCH_REPORT.md`](oracle/ORACLE_RESEARCH_REPORT.md) | What does the Oracle-generation literature and industry practice establish? | Research basis, non-normative |
| [`oracle/BORROWABLE_DIRECTIONS.md`](oracle/BORROWABLE_DIRECTIONS.md) | Which external ideas are worth adapting, and which should not be copied? | Research synthesis, non-normative |

## Reading order

1. Read repository [`../AGENTS.md`](../AGENTS.md).
2. Read the current product and focused SIR/Oracle documents above.
3. Read the requirements and system design for obligations not explicitly updated by the current
   documents.
4. Read [`design/README.md`](design/README.md) for code, logical, and runtime architecture.
5. Read [`dev/README.md`](dev/README.md) and the current implementation baseline before planning a
   slice; design completion is not implementation evidence.
6. Use `oracle/`, durable execution records, decisions and open questions as focused rationale and
   guardrails without restoring superseded fixed-stage timing.

## Normative language

`MUST`, `MUST NOT`, `SHOULD`, `SHOULD NOT`, and `MAY` are used in their ordinary requirements sense.
Each normative requirement has a stable identifier. A design section may explain how a requirement
is expected to be met, but passing prose is not acceptance evidence.

## Document maintenance rules

1. Requirements state observable properties, not implementation aspirations.
2. Design documents distinguish **target**, **implemented**, and **measured**. In this initial
   baseline, everything is target unless it explicitly cites evidence from the new repository.
3. A changed trust boundary or externally visible protocol requires both a requirements review and a
   design update.
4. A rejected alternative belongs beside the decision that rejected it. Historical narrative and
   experiment costs belong in future `docs/evidence/` records.
5. Open questions are not silently resolved in code. Resolve one by updating the relevant normative
   document and recording the evidence or argument used.
6. Old Cairn and Alloyport behavior may become regression evidence, but old names, aggregates, and
   deployment accidents are not automatically requirements.

### Conflict and precedence rule

Requirements define observable obligations; accepted decisions record chosen policy; the system
design defines overall authority/dependency; focused designs add subsystem detail without weakening
those obligations. The implementation plan, current code, historical fixtures, research reports, and
open questions cannot override normative design. A real conflict between normative documents blocks
the affected work until all impacted documents are reconciled; it is not permission to implement a
fallback or choose the most convenient interpretation. Oracle-specific conformance is summarized in
[`oracle/DESIGN_INVARIANTS.md`](oracle/DESIGN_INVARIANTS.md).

The current overlay above is an explicit current-V1 update, not an accidental conflict or a
compatibility layer. It changes SIR/Oracle/Candidate timing while preserving authority, evidence,
target, hidden-control and mechanical Admission obligations.

## Source material

The baseline was derived from:

- the old Alloyport product, architecture, oracle-calibration evidence, and final Claude Code
  discussion;
- the old Cairn record, provider-substitution replay, input audit, and counterfactual controls;
- the newly open-source DeepSeek Harness architecture, especially durable session events and
  capability seams;
- the open-source OpenAI Codex core/App Server separation and bidirectional agent protocol.

Those projects are references. Cairn's trust model and product boundary are defined here.
