# Oracle proposal, admission, and verdict design

- Status: normative focused design
- Date: 2026-08-24
- Parent design: [`SYSTEM_DESIGN.md`](SYSTEM_DESIGN.md)
- Requirements: `FR-ORACLE-*`, `QR-AUD-*`

## 1. Purpose

An oracle is the instrument that turns observations into a claim about a candidate. If the
instrument cannot distinguish a correct implementation from an incorrect one, a precise-looking
verdict is worse than no verdict.

Cairn therefore treats an oracle as a searched, attacked, executed, versioned, and admitted artifact.
An author—human or model—may propose an oracle. Only trusted admission code may grant that proposal
the right to judge a candidate, and only for the scope the admission evidence supports.

The central rule is:

> The model may define proposed semantics. The model may not decide whether its proposed semantics
> and tests are capable of separating right from wrong.

## 2. What an oracle claim contains

An oracle claim is not a boolean function. It includes:

- **subject** — the implementation/artifact being judged;
- **domain** — inputs, shapes, dtypes, ranges, error behavior, and exclusions;
- **semantics** — reference results, allowed result sets, properties, or implicit checks;
- **observation plan** — how executions become typed observations;
- **allowance policy** — numerical or nondeterministic variation that correct implementations may
  occupy;
- **coverage obligations** — cases and failure modes that must be exercised;
- **strength** — reference, property/metamorphic, implicit, or unavailable;
- **assumptions** — independence, device execution, runner integrity, source authority, and similar
  conditions;
- **admission evidence** — honest paths, attacks, mutants, blind spots, and execution scope.

Changing any of these creates a new oracle/experiment identity.

## 3. Three separate comparisons

The initial numerical case exposed three questions that must remain separate:

| Comparison | Question | Output |
|---|---|---|
| candidate vs semantic reference/property | Is the candidate consistent with the requested semantics? | candidate verdict observations |
| valid implementation family vs semantic reference/property | How much legitimate variation may a correct implementation occupy? | measured allowance and false-reject control |
| implementation vs its own repetitions | Is this implementation stable under the declared repetition policy? | determinism/nondeterminism claim |

One execution may produce data used by more than one comparison, but the claims, policies, and
receipts remain independently identifiable.

### 3.1 Why the source implementation is not the numeric baseline

The source implementation is one executable realization. Its floating-point rounding is a sample,
not the center of all correct target implementations. A tolerance centered only on its result can be
narrower than the space occupied by two correct evaluation orders.

For operations where a higher-precision reference is meaningful, the candidate is judged against
that reference or allowed-result set. Source fp behavior helps measure legitimate implementation
spread and admit the proposed semantics; it does not by itself define the only correct byte pattern.

### 3.2 “High precision” is a claim, not a universal fact

`f64` may provide an effectively exact reference for some fp32 reductions. It is not exact for every
transcendental, discontinuous operation, tie-bearing selection, or higher-precision input. The oracle
artifact records reference strength rather than naming every host reference `exact`.

## 4. Roles and non-circularity

### 4.1 Roles

| Role | Objective | May produce | May not decide |
|---|---|---|---|
| Blue/oracle author | propose what correct means | domain refinements, reference, properties, valid-family sources | admission or candidate verdict |
| Red/oracle breaker | expose false accepts and false rejects | correct-by-construction variants, deliberately wrong variants, adversarial cases | admission outcome |
| Candidate author | produce a target implementation | candidate bundle | oracle, allowance, corpus, comparison, verdict |
| Trusted verifier | test the proposed instrument | generic mutations, derivation, comparison, adjudication | operator semantics not supported by evidence |

Roles are capability scopes. They may use different models, the same model in separate episodes, or
human-authored artifacts. Different model families reduce shared-prior correlation but do not turn a
model into trusted adjudication.

### 4.2 Correct-by-construction evidence

A red variant is a false-reject control only when its correctness is justified independently of the
oracle under test. Examples include transformations with a structural argument:

- a different legal accumulation order;
- a different parallel decomposition;
- a compensated summation variant where the contract permits it;
- block decomposition and recomposition;
- algebraically or definitionally equivalent transformations under declared numerical semantics;
- transpose/partition transformations with independently checked shape rules.

“The proposal passes the oracle” is never its correctness argument. Each variant cites a
`ConstructionClaim` with transformation kind, prerequisites, source artifact, and any proof or
independent control.

### 4.3 Opposed incentives, not only blindness

Blue and candidate isolation prevents an oracle from being tailored to a particular candidate.
Red adds an opposed objective: find an incorrect implementation that passes and a correct
implementation that fails. Both mandates are required. Testing only false accepts admits an oracle
that rejects every candidate; testing only false rejects admits an oracle that accepts everything.

## 5. Oracle artifact model

### 5.1 Proposal bundle

An `OracleProposal` canonically cites:

- task and declared domain identities;
- proposed domain refinements, with differences from the caller declaration;
- corpus proposal and derivation provenance;
- reference implementation(s) or allowed-result-set implementation;
- property/metamorphic relations;
- source-admission plan;
- valid-family generation plan;
- observation ABI and result schema;
- requested oracle strength;
- author/model/configuration provenance.

It does not carry trusted mutants, tolerance derivation, comparison policy, or an admission result.

### 5.2 Trusted admission policy

An `AdmissionPolicy` canonically cites:

- applicable generic mutant set and versions;
- minimum correct and incorrect variant counts;
- required construction/fault classes and structural-independence rules;
- saturation rounds, reset conditions, and budget-exhaustion outcome;
- tolerance/allowance derivation policy;
- comparison and adjudication implementations;
- coverage rules;
- minimum honest-path and attack requirements;
- required execution scope;
- resource/sandbox policy;
- fatal-miss classification;
- accepted oracle strengths and fallback behavior.

Proposal and policy are separate identities. A proposal cannot widen its own admission policy.
The V1 reference profile starts with two structurally distinct correct variants, three deliberately
incorrect variants, applicable fault-class coverage, and two consecutive saturation rounds. These
values are versioned profile configuration rather than hard-coded verifier behavior. Counts do not
replace construction claims, execution-scope checks, or the generic mutation grid. Exhausting a
budget before the policy is satisfied cannot produce `Pass`.

### 5.3 Admitted oracle

An `AdmittedOracle` is an immutable manifest citing:

- the exact proposal version;
- the exact admission policy;
- the complete calibration/admission receipt;
- admitted domain and exclusions;
- admitted strength;
- allowance policy produced by admission;
- frozen corpus;
- known blind spots, assumptions, and unverified claims;
- expiration/revalidation policy, if any.

It contains no mutable “latest calibration” pointer. A newer admission produces a new artifact.

## 6. Lifecycle

```text
Draft
  → Proposed
  → InputsResolved
  → AttacksResolved
  → AdmissionRunning
      → Rejected
      → Unverifiable
      → Admitted
  → Frozen
  → Superseded (historical verdicts still cite the old version)
```

`Rejected` means the instrument contradicted a required admission check and may be corrected.
`Unverifiable` means the requested claim strength could not be established from available evidence.
Neither is a candidate verdict.

Admission attempts are immutable. A correction creates a new proposal version and a new attempt.

## 7. Domain and corpus admission

### 7.1 Sources of domain knowledge

Cairn preserves separately:

1. caller-declared supported domain;
2. blue-proposed structured interpretation;
3. source implementation's observed behavior;
4. upstream definitions/tests and external proposals;
5. target-specific coverage requirements learned from historical failures.

No source silently overwrites another. Disagreement is evidence.

Under [`D-003`](DECISIONS.md#d-003--hybrid-authority-for-the-structured-domain), the caller must
supply the minimum structured contract from which mandatory base boundaries can be derived. Blue may
propose refinements, but each refinement cites evidence and remains a proposal until admission.
Source interrogation and external/upstream cases challenge both. Admission emits a separate immutable
admitted-domain artifact; it does not edit any source's statement in place.

### 7.2 Derived base corpus

Trusted code derives mandatory base cases from the structured declaration:

- minimum, maximum, inside/outside boundary values;
- zero, one, empty, singleton, and degenerate shapes as applicable;
- tile/alignment boundaries and tails;
- dtype extrema, signed zero, non-finite, denormal, cancellation, and scale patterns where relevant;
- declared invalid pointers, sizes, shapes, and status behavior;
- target-failure coverage obligations attached to the domain family.

Blue, red, upstream tests, OpInfo-like sources, and fuzzing may add proposed cases. Proposed cases do
not become truth by source. They enter admission with provenance and license metadata.

### 7.3 Real-domain interrogation

The source artifact is executed across boundary, fuzzed, coverage-guided, and adversarially
constructed inputs where authorized. This can reveal:

- declared domain broader than observed behavior;
- source defects;
- error/status surfaces;
- alignment and shape discontinuities;
- numerical error surfaces;
- cases that silently produce invalid data.

The source cannot lie about what it did, but it can be wrong relative to the intended semantics.
Consequently its observations are behavioral authority, not unconditional semantic authority.

### 7.4 Coverage claim

Coverage is a first-class claim with:

- obligations required;
- obligations exercised;
- generation/selection method;
- unexplored regions;
- coverage metrics and their limitations;
- relationship to historical failure classes.

Random case count alone is not a coverage claim.

## 8. Reference, property, and implicit strengths

### 8.1 Reference strength

Requires a reference or allowed-result-set computation admitted against independent evidence.
Numerical allowance is measured from valid implementations and adversarial inputs where needed.

### 8.2 Property/metamorphic strength

Used when a direct result reference is unavailable or insufficient. A relation is itself a proposal.
Admission executes it against source behavior, correct variants, incorrect variants, and applicable
generic mutations. Numerical metamorphic relations carry their own measured allowance.

### 8.3 Implicit strength

Checks only properties not requiring a semantic result oracle, such as:

- invocation occurred;
- process/device status;
- no crash/hang under policy;
- output shape/format contract;
- finite output where declared;
- repetition/determinism behavior.

An implicit pass cannot be presented as semantic correctness.

### 8.4 Unavailable strength

When no requested strength can be admitted, the result is `Unverifiable` with failed proof
obligations and possible weaker claims. A caller may start a new task requesting a weaker claim; it
does not rewrite the original request.

## 9. Numerical allowance

### 9.1 Provenance

An allowance has one of these provenance classes:

- `MeasuredFamily` — derived from executed correct-by-construction variants;
- `MeasuredAdversarial` — derived from search for high-error valid inputs;
- `ExternalPrior` — imported convention such as a dtype default, not sufficient alone for strong
  admission;
- `Asserted` — proposed number without measurement, not admissible for a measured numeric claim;
- `ExactOrSet` — exact/set membership where arithmetic and specification justify it.

Provenance answers where a number came from. It does not state how far that evidence generalizes.

### 9.2 Assurance

Allowance assurance is classified independently:

- `ProvenBound` — a mathematical or structural argument justifies the bound;
- `ExhaustiveFinite` — every member of a declared finite domain was exercised;
- `HeldOutValidated` — identity-disjoint derivation and validation corpora support an empirical
  allowance;
- `ExploratoryMeasured` — executed samples provide observations but no independent validation;
- `PriorOnly` — only an imported convention supports the allowance;
- `Unsupported` — no admissible support exists.

`HeldOutValidated` may produce an empirical `Pass` only when its policy allows it. The verdict names
the empirical status, admitted domain, corpus relationship, and unexplored regions. Only
`ProvenBound` and `ExhaustiveFinite` may support an unqualified domain-wide numerical claim. Safety
factors do not promote measured maxima to proven bounds, and Cairn reports no probabilistic
confidence without a declared distribution and justified sampling procedure.

### 9.3 Per-case or per-region policy

Allowance is associated with a case or a justified domain region. A single global tolerance requires
evidence that it is appropriate across the domain. Magnitude, condition number, shape, and operation
count may change legitimate fp spread by orders of magnitude.

### 9.4 Valid-family spread

Correct-by-construction variants establish false-reject controls and a lower bound on room correct
implementations need. One variant and one case are samples, not a family. Admission records family
size, independence/derivation, input construction, observed span, seeds, and unexplored regions.
Evidence used to derive a `HeldOutValidated` allowance cannot also serve as its held-out validation.

### 9.5 Self-validation prohibition

If measurement `M` derives threshold `T`, replaying `M` against `T` is not independent admission
evidence. For example:

- a distance used to set tolerance cannot by itself prove that tolerance accepts correct diversity;
- a mutant sized just outside tolerance proves the comparator enforces `T`, not that `T` is sound;
- comparing a reference to itself proves data-path identity, not correctness.

Each derivation records the independent control intended to test it.

## 10. Attack and mutation model

### 10.1 Executed red variants

Red produces:

- correct-by-construction variants for false-reject detection and valid-family spread;
- deliberately wrong variants for false-accept detection;
- adversarial input proposals for domain and error-surface exploration.

Variants are compiled and executed through the same observation path used for candidate judgment.
Forging their expected observations directly into receipts is only a comparator unit test.

### 10.2 Generic trusted mutants

The initial generic families include, where applicable:

- arithmetic scale/offset;
- zeroing or masking output regions;
- indexing/permutation errors;
- status/error-code corruption;
- signed-zero corruption;
- non-finite injection;
- determinism/repetition corruption;
- boundary omission or off-by-one;
- announced-boundary plus one step;
- invocation/observation-path omission.

Operator-specific mutants may be accepted only as trusted verification code with explicit review and
provenance. Prefer deriving target failure coverage from real records over imagining an exhaustive
list.

### 10.3 Mutation sizing

Each applicable mutant/case trial is classified:

- `PolicySized` — magnitude derived from the announced boundary; any miss is fatal because the
  comparator failed its own contract;
- `ScaleFree` — invariantly destructive or outside numeric allowance; applicable miss is fatal;
- `CaseDependent` — fixed/context-dependent defect may fit within legitimate allowance; a miss is a
  non-fatal but mandatory blind spot;
- `NotInjectable` — trial cannot be constructed for this case, with reason.

The receipt contains the full grid, not only an `undetected` summary. Admission recomputes fatal
misses from trials. An empty applicable grid cannot pass.

### 10.4 Honest path first

Before tightening an admission policy, Cairn runs it over archived honest source/reference receipts
and correct variants. A policy that catches more mutants by rejecting established legitimate
behavior has not improved.

## 11. Executed admission algorithm

For an admission attempt, trusted orchestration performs:

1. validate schemas, identities, provenance, license, and role origin;
2. derive mandatory domain/corpus obligations;
3. build proposal reference/property artifacts on CPU;
4. execute reference/property self-consistency checks;
5. compile and run correct-by-construction variants at the cheapest applicable tiers;
6. derive candidate allowance only from permitted evidence;
7. require every correct variant to pass;
8. compile and run deliberately wrong variants;
9. require every mandatory wrong variant to fail;
10. run the full generic mutant/case grid through the implementation observation path;
11. execute source implementation admission and domain interrogation;
12. adjudicate source/reference/case disagreements without silent deletion;
13. evaluate domain and historical-failure coverage;
14. recompute fatal misses and blind spots from underlying trials;
15. emit `Rejected`, `Unverifiable`, or an immutable `AdmittedOracle`.

The order within V0/V1 may be optimized, but no expensive tier runs after a cheaper decisive
failure. Every skipped check is recorded with reason.

## 12. Admission receipt

The canonical receipt contains at least:

- proposal, task, domain, corpus, policy, environment, and source identities;
- requested and admitted oracle strength;
- execution scope: comparator, observation pipeline, implementation, and hardware tiers actually
  exercised;
- correct-variant trials and construction claims;
- wrong-variant trials;
- generic mutation grid;
- allowance derivation evidence;
- source admission observations;
- coverage obligations and results;
- blind spots and non-injectable cells;
- disagreements and adjudication claims;
- assumptions and unverified facts;
- decision plus machine-readable failed proof obligations.

The stored decision is convenient metadata. Readers recompute the decision from trials and policy
where feasible, and fail closed when required underlying evidence is missing.

## 13. Multi-source disagreement

### 13.1 Three-way observation

For a proposed case, keep separate outcomes from the case expectation, source implementation, and
reference/property artifact. Typical patterns are:

| Case expectation | Source | Reference | Interpretation |
|---|---|---|---|
| inconsistent | consistent | consistent | case proposal is suspect |
| consistent | inconsistent | consistent | source implementation defect or domain mismatch is suspect |
| consistent | consistent | inconsistent | proposed reference/property is suspect |
| inconsistent | inconsistent | consistent | case may be outside declared domain or both share another issue |

“Two against one” localizes a suspect under independence assumptions; it is not unconditional proof.
The receipt names shared authorship, derivation, provider family, common code, and other correlations
that weaken the inference.

### 13.2 No automatic victim/suspect rule

Cairn does not default to “candidate is wrong,” “test is wrong,” or “source is authoritative.” It
records the disagreement and applies an explicit policy supported by independent controls. An
unresolved disagreement rejects or weakens admission.

## 14. Candidate judgment

An admitted oracle judges only:

- the frozen task/domain/corpus;
- the admitted strength and allowance;
- environments allowed by its policy;
- candidate observations produced through the admitted observation path.

A candidate verdict receipt cites the admitted oracle, candidate, source/build/run receipts, and all
failed cases. It carries oracle blind spots and unverified assumptions forward; a `Pass` cannot make
them disappear.

Candidate failure does not retroactively prove the oracle correct. Conversely, an oracle admission
does not prove every future candidate-path integration is correct; target executions still require
integrity and observation controls.

## 15. Target-specific failure coverage

CPU/source admission validates semantics and numerical method. It cannot by itself prove coverage of
target-specific failures. Cairn maintains versioned coverage requirements derived from real
migration records, initially including classes such as:

- target address-space/pointer typing;
- narrowing in target copy/tiling descriptors;
- initialization and runtime setup;
- non-aligned tails and non-tile-multiple shapes;
- device selection/visibility mapping;
- build/link/runtime library boundaries;
- output capture and invocation proof.

Each requirement cites the historical event/receipt that motivated it. It enters a new oracle policy
version through review and controls; it does not mutate past admissions.

## 16. Oracle evolution and retraction

### 16.1 Supersession

New evidence may produce a new proposal, corpus, policy, or admission. The new oracle supersedes the
old for future tasks but does not rewrite historical verdict bytes.

### 16.2 Impact analysis

When a defect is found, Cairn walks the identity graph to list verdicts depending on the affected
oracle/policy/artifact. Each receives an impact claim:

- unaffected, with reason;
- weakened but still valid at a lower strength;
- requires re-evaluation;
- invalidated/retracted.

Retraction is adjudicated from evidence, not triggered merely because a newer version exists.

### 16.3 Hand-written parity

Repository-authored oracles are not grandfathered. The first implementation milestone runs the old
hand-written reduction oracle through the same executed admission intended for model proposals.

## 17. Threats to validity

Admission MUST report at least these risks when applicable:

- shared model/provider priors among blue, red, and candidate;
- blue both interpreting the domain and writing the reference;
- reference derived from the same source it is meant to check;
- source implementation and external corpus sharing an upstream bug;
- insufficient correct-variant family size;
- sampled rather than adversarial input regions;
- missing target-specific coverage;
- comparator correct but observation pipeline untested;
- device execution or runner binary not independently attested;
- metamorphic relation admitted only over a narrow sample;
- source and target using different input bytes or environment assumptions.

These are typed assumptions/limitations in the receipt, not prose appended only to a report.

## 18. First implementation control

The first admission implementation is accepted only when it can demonstrate all of the following
without a paid provider turn or target device:

1. load the historical reduction domain/reference/corpus as an ordinary proposal;
2. reproduce the historical false reject under the old single-sample allowance;
3. accept the correct tree-reduction candidate under measured valid-family spread;
4. compile and execute the variants required by the selected reference profile, initially at least
   two structurally correct and three deliberately wrong variants;
5. make a known wrong variant red through the complete build/execute/observe/compare path;
6. retain the known case-dependent accumulation blind spots;
7. reject an asserted/unmeasured allowance;
8. reject an empty mutation grid;
9. show that tampering with stored `passed` metadata cannot override underlying trials;
10. emit an admitted oracle whose identity graph is complete.

Only after this control should a model-authored oracle proposal be allowed to judge a target
candidate.
