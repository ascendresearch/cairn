# Cairn design baseline

- Status: initial target design
- Date: 2026-08-24
- Scope: the new, unified Cairn repository

This directory is the authoritative design baseline for the rewrite. It replaces neither the old
repositories' evidence nor their histories. It separates current decisions from historical
narrative so that a future implementation cannot be mistaken for complete merely because a design
document is complete.

## Document map

| Document | Question it answers | Authority |
|---|---|---|
| [`SYSTEM_REQUIREMENTS.md`](SYSTEM_REQUIREMENTS.md) | What must the system do, refuse, expose, and prove? | Normative requirements |
| [`SYSTEM_DESIGN.md`](SYSTEM_DESIGN.md) | What architecture is intended to satisfy those requirements? | Normative target design |
| [`ORACLE_ADMISSION.md`](ORACLE_ADMISSION.md) | Why may an oracle judge a candidate? | Normative verification design |
| [`RECORD_REPLAY.md`](RECORD_REPLAY.md) | What is recorded, reconstructed, replayed, and compared? | Normative record design |
| [`DECISIONS.md`](DECISIONS.md) | Which formerly open choices have been resolved? | Normative decision register |
| [`OPEN_QUESTIONS.md`](OPEN_QUESTIONS.md) | What has not been decided? | Explicitly non-normative decision backlog |
| [`RELEASE.md`](RELEASE.md) | How are x86-64/AArch64 artifacts cross-linked, inspected, and promoted? | Normative release procedure |
| [`ENROLLMENT.md`](ENROLLMENT.md) | How does a worker bootstrap a local managed identity without copied private keys? | Operator procedure and implemented trust boundary |
| [`SCHEDULER.md`](SCHEDULER.md) | How are generic candidates selected, reserved, assigned, retried, and safely released? | Implemented scheduling trust boundary |
| [`RESOURCE_PROBING.md`](RESOURCE_PROBING.md) | Which resource facts are observed, configured, matched, and still deferred? | Implemented D1 probe/operator contract |
| [`OCI_CONTAINER_SECURITY.md`](OCI_CONTAINER_SECURITY.md) | What must the CPU-only OCI backend hide, bind, recover, and refuse? | F2d security boundary and implementation status |
| [`IMPLEMENTATION_PLAN.md`](IMPLEMENTATION_PLAN.md) | In what dependency order do worker authority, scheduling, probing, registry lifecycle, and onboarding close? | Active delivery plan and acceptance gates |

## Reading order

1. Read the requirements to understand the product boundary and acceptance properties.
2. Read the system design for the architecture and end-to-end workflows.
3. Read the two focused designs for the system's defining guarantees: oracle admission and durable
   execution evidence.
4. Read resolved decisions and open questions before changing an unsettled boundary.

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

## Source material

The baseline was derived from:

- the old Alloyport product, architecture, oracle-calibration evidence, and final Claude Code
  discussion;
- the old Cairn record, provider-substitution replay, input audit, and counterfactual controls;
- the newly open-source DeepSeek Harness architecture, especially durable session events and
  capability seams;
- the open-source OpenAI Codex core/App Server separation and bidirectional agent protocol.

Those projects are references. Cairn's trust model and product boundary are defined here.
