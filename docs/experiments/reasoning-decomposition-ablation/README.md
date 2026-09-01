# Reasoning-decomposition ablation

This directory preserves the A/B/C reasoning-decomposition experiment independently of transient
runtime state.

The three treatment arms are:

- **A — `MinimalDecomposition`**: one SIR episode and one whole-portfolio Oracle episode;
- **B — `StructuredReview`**: dimension/item discovery, independent Review and revision loops;
- **C — `EvidenceAugmentedStructuredReview`**: B plus proposal-Agent-visible typed Worker
  experiments and exact receipt feedback.

All arms must use the normal `cairn-cli -> cairn-server -> migration app API ->
CudaMigrationWorkflow` path. Final hidden evaluation may execute external controls for every arm;
only C may expose newly requested experiment observations to the proposal Agent.

## Status

The preserved A, B and C results are **pilots**, not formal paired ablation observations. B's
implementation, prompt and read limits were not frozen before execution, and it was intentionally
interrupted after revealing a false structural Oracle-qualification path. A used the frozen pilot
limits and reached the expected `OracleMechanisms` fail-closed boundary. C used those limits and
demonstrated typed Worker evidence, but was interrupted after first-dimension item explosion and a
pending revision Review. There is still no common hidden semantic evaluator; the pilots are not
causally comparable.

- [Cross-arm pilot comparison](COMPARISON.md)
- [A pilot result](pilots/A-simplePitchLinearTexture/RESULT.md)
- [A completeness audit](pilots/A-simplePitchLinearTexture/COMPLETENESS.md)
- [B pilot result](pilots/B-simplePitchLinearTexture/RESULT.md)
- [B completeness audit](pilots/B-simplePitchLinearTexture/COMPLETENESS.md)
- [B submitted domain artifacts](pilots/B-simplePitchLinearTexture/submitted-artifacts)
- [C pilot result](pilots/C-simplePitchLinearTexture/RESULT.md)
- [C completeness audit](pilots/C-simplePitchLinearTexture/COMPLETENESS.md)
- [C submitted domain artifacts](pilots/C-simplePitchLinearTexture/submitted-artifacts)
- [C derived domain artifacts](pilots/C-simplePitchLinearTexture/derived-domain-artifacts)

A formal experiment manifest will be frozen here before comparable A, B and C repetitions begin.
The pilot remains valuable for qualitative findings and implementation debugging but must never be
mixed into the formal effect estimate.

The proposed follow-up architecture is documented in
[Blind-First, Policy-Challenged Oracle Scope Design](../../design/BLIND_FIRST_ORACLE_SCOPE_DESIGN.md).
It is a candidate D treatment, not an implementation fact or a conclusion that the A/B/C pilots
causally established its superiority.

Its candidate metric definitions, denominators, failure handling and reporting rules are in
[D measurement protocol](D_MEASUREMENT_PROTOCOL.md). That protocol is not yet a frozen formal
manifest; it must be preregistered together with the evaluator and treatment identities before any
causal run.

A subsequent first-principles review produced
[Evidence-Driven Adaptive Migration Co-Design](../../design/EVIDENCE_DRIVEN_ADAPTIVE_MIGRATION_DESIGN.md),
candidate treatment E. E does not assume a sufficiently strong model and does not add a mini-SIR
classifier. It lets intent, assurance and an authority-restricted exploratory candidate co-evolve,
then requires a late sealed-policy challenge, immutable qualification epoch and the same mechanical
release gates. Candidate promotion additionally requires a predeclared improvement, required
non-regression, same-epoch parent/current comparison and independent qualification. Oracle changes
invalidate the epoch and require symmetric replay; detailed hidden feedback retires the affected
control to public and requires replacement. D remains both an independent up-front comparison and
E's full-structure fallback.

E's proposed metrics extend, rather than replace, the D protocol. No D/E treatment or manifest is
frozen yet, and neither candidate has been established as superior.
