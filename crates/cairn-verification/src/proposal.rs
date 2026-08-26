//! Immutable oracle proposal inputs and provenance graph.

use std::{fmt, str::FromStr};

use cairn_protocol::{ContentId, ContentType, EpisodeId, TaskId};
use serde::{Deserialize, Deserializer, Serialize, de};

use super::{
    ConstructionClassName, FaultClassName, OracleStrength, VerificationContractError,
    VerificationSchemaV1, validate_content_id_order, validate_label,
};

macro_rules! content_artifact {
    ($(#[$meta:meta])* $name:ident, $domain:literal) => {
        $(#[$meta])*
        pub enum $name {}

        impl ContentType for $name {
            const DOMAIN: &'static str = $domain;
        }
    };
}

content_artifact!(
    /// Frozen task inputs visible to oracle proposal and admission.
    OracleTaskInputArtifact,
    "verification.oracle-task-input.v1"
);
content_artifact!(
    /// Product/domain-adapter body of the caller's structured domain declaration.
    CallerDomainBodyArtifact,
    "verification.caller-domain-body.v1"
);
content_artifact!(
    /// Caller-side provenance supporting the declared domain.
    CallerDomainEvidenceArtifact,
    "verification.caller-domain-evidence.v1"
);
content_artifact!(
    /// One explicitly unresolved domain fact.
    DomainUnknownArtifact,
    "verification.domain-unknown.v1"
);
content_artifact!(
    /// Canonical caller-domain manifest.
    DeclaredDomainArtifact,
    "verification.declared-domain.v1"
);
content_artifact!(
    /// Exact structured difference proposed against the caller declaration.
    DomainDifferenceArtifact,
    "verification.domain-difference.v1"
);
content_artifact!(
    /// Evidence cited by a domain refinement, distinct from construction evidence.
    DomainRefinementEvidenceArtifact,
    "verification.domain-refinement-evidence.v1"
);
content_artifact!(
    /// Evidence supporting a correct-by-construction argument.
    ConstructionEvidenceArtifact,
    "verification.construction-evidence.v1"
);
content_artifact!(
    /// Canonical domain-refinement manifest.
    DomainRefinementArtifact,
    "verification.domain-refinement.v1"
);
content_artifact!(
    /// One proposed executable corpus case.
    CorpusCaseArtifact,
    "verification.corpus-case.v1"
);
content_artifact!(
    /// Source provenance for one proposed corpus case.
    CorpusCaseProvenanceArtifact,
    "verification.corpus-case-provenance.v1"
);
content_artifact!(
    /// Provenance for one proposed corpus case.
    LicenseProvenanceArtifact,
    "verification.license-provenance.v1"
);
content_artifact!(
    /// One required domain or historical-failure coverage obligation.
    CoverageObligationArtifact,
    "verification.coverage-obligation.v1"
);
content_artifact!(
    /// Canonical corpus proposal before admission freezes a corpus.
    CorpusProposalArtifact,
    "verification.corpus-proposal.v1"
);
content_artifact!(
    /// Model template/configuration provenance for authored proposal material.
    ModelConfigurationArtifact,
    "verification.model-configuration.v1"
);
content_artifact!(
    /// Immutable implementation source/bundle used by a variant.
    ImplementationBundleArtifact,
    "verification.implementation-bundle.v1"
);
content_artifact!(
    /// One independently recorded prerequisite for a construction claim.
    ConstructionPrerequisiteArtifact,
    "verification.construction-prerequisite.v1"
);
content_artifact!(
    /// Canonical correct-by-construction claim.
    ConstructionClaimArtifact,
    "verification.construction-claim.v1"
);
content_artifact!(
    /// Canonical correct or deliberately incorrect implementation variant.
    ImplementationVariantArtifact,
    "verification.implementation-variant.v1"
);
content_artifact!(
    /// Evidence proving that a declared fault was injected into a wrong variant.
    FaultInjectionEvidenceArtifact,
    "verification.fault-injection-evidence.v1"
);
content_artifact!(
    /// Proposed semantic reference or allowed-result-set implementation.
    ReferenceArtifact,
    "verification.reference-proposal.v1"
);
content_artifact!(
    /// Proposed property or metamorphic relation.
    PropertyRelationArtifact,
    "verification.property-relation-proposal.v1"
);
content_artifact!(
    /// Proposed source interrogation/admission plan.
    SourceAdmissionPlanArtifact,
    "verification.source-admission-plan.v1"
);
content_artifact!(
    /// Proposed valid-family generation plan.
    ValidFamilyPlanArtifact,
    "verification.valid-family-plan.v1"
);
content_artifact!(
    /// Proposed observation ABI and typed result schema.
    ObservationPlanArtifact,
    "verification.observation-plan.v1"
);
content_artifact!(
    /// Canonical oracle proposal bundle. It is not an admitted oracle.
    OracleProposalArtifact,
    "verification.oracle-proposal.v1"
);

macro_rules! proposal_label {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a validated proposal label.
            ///
            /// # Errors
            ///
            /// Rejects empty, oversized, or non-canonical labels.
            pub fn new(value: impl Into<String>) -> Result<Self, VerificationContractError> {
                let value = value.into();
                validate_label(&value, $kind)?;
                Ok(Self(value))
            }

            /// Returns the canonical wire label.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = VerificationContractError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

proposal_label!(
    /// Stable provenance label for a human, model deployment, repository component, or source.
    ArtifactAuthorId,
    "artifact author identity"
);
proposal_label!(
    /// Named transformation applied by a correct-by-construction variant.
    TransformationKindName,
    "transformation kind"
);

/// Origin of authored proposal material. Origin records provenance but grants no trust.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthorshipOrigin {
    /// Caller-authored input.
    Caller,
    /// Human-authored proposal material.
    Human,
    /// Model-authored proposal material.
    Model,
    /// Repository-authored material subject to the same admission controls.
    Repository,
    /// External/upstream source.
    External,
}

/// Authorship provenance retained without converting origin into trust.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "ArtifactAuthorshipWire")]
pub struct ArtifactAuthorshipV1 {
    schema_version: VerificationSchemaV1,
    origin: AuthorshipOrigin,
    author_id: ArtifactAuthorId,
    episode_id: Option<EpisodeId>,
    model_configuration: Option<ContentId<ModelConfigurationArtifact>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactAuthorshipWire {
    schema_version: VerificationSchemaV1,
    origin: AuthorshipOrigin,
    author_id: ArtifactAuthorId,
    episode_id: Option<EpisodeId>,
    model_configuration: Option<ContentId<ModelConfigurationArtifact>>,
}

impl ArtifactAuthorshipV1 {
    /// Creates explicit authorship provenance.
    ///
    /// # Errors
    ///
    /// Model authorship requires both a durable episode and exact model configuration. Other
    /// origins cannot smuggle a model configuration under a non-model label.
    pub fn new(
        origin: AuthorshipOrigin,
        author_id: ArtifactAuthorId,
        episode_id: Option<EpisodeId>,
        model_configuration: Option<ContentId<ModelConfigurationArtifact>>,
    ) -> Result<Self, VerificationContractError> {
        if origin == AuthorshipOrigin::Model
            && (episode_id.is_none() || model_configuration.is_none())
        {
            return invalid(
                "artifact authorship",
                "model origin requires episode and model configuration provenance",
            );
        }
        if origin != AuthorshipOrigin::Model && model_configuration.is_some() {
            return invalid(
                "artifact authorship",
                "non-model origin cannot cite model configuration provenance",
            );
        }
        Ok(Self {
            schema_version: VerificationSchemaV1,
            origin,
            author_id,
            episode_id,
            model_configuration,
        })
    }

    /// Returns the recorded origin without assigning it trust.
    #[must_use]
    pub const fn origin(&self) -> AuthorshipOrigin {
        self.origin
    }

    /// Returns the stable author/source label.
    #[must_use]
    pub const fn author_id(&self) -> &ArtifactAuthorId {
        &self.author_id
    }

    /// Returns the authoring episode when one exists.
    #[must_use]
    pub const fn episode_id(&self) -> Option<EpisodeId> {
        self.episode_id
    }

    /// Returns exact model configuration provenance for model-authored material.
    #[must_use]
    pub const fn model_configuration(&self) -> Option<ContentId<ModelConfigurationArtifact>> {
        self.model_configuration
    }
}

impl TryFrom<ArtifactAuthorshipWire> for ArtifactAuthorshipV1 {
    type Error = VerificationContractError;

    fn try_from(wire: ArtifactAuthorshipWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(
            wire.origin,
            wire.author_id,
            wire.episode_id,
            wire.model_configuration,
        )
    }
}

/// Caller domain manifest retaining explicit unknowns and provenance separately from refinements.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "DeclaredDomainWire")]
pub struct DeclaredDomainV1 {
    schema_version: VerificationSchemaV1,
    task_id: TaskId,
    body: ContentId<CallerDomainBodyArtifact>,
    explicit_unknowns: Vec<ContentId<DomainUnknownArtifact>>,
    caller_evidence: ContentId<CallerDomainEvidenceArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeclaredDomainWire {
    schema_version: VerificationSchemaV1,
    task_id: TaskId,
    body: ContentId<CallerDomainBodyArtifact>,
    explicit_unknowns: Vec<ContentId<DomainUnknownArtifact>>,
    caller_evidence: ContentId<CallerDomainEvidenceArtifact>,
}

impl DeclaredDomainV1 {
    /// Creates a caller declaration without merging later observations or refinements into it.
    ///
    /// # Errors
    ///
    /// Rejects explicit unknown identities that are duplicated or out of canonical order.
    pub fn new(
        task_id: TaskId,
        body: ContentId<CallerDomainBodyArtifact>,
        explicit_unknowns: Vec<ContentId<DomainUnknownArtifact>>,
        caller_evidence: ContentId<CallerDomainEvidenceArtifact>,
    ) -> Result<Self, VerificationContractError> {
        validate_content_id_order(&explicit_unknowns, "explicit domain unknowns")?;
        Ok(Self {
            schema_version: VerificationSchemaV1,
            task_id,
            body,
            explicit_unknowns,
            caller_evidence,
        })
    }

    /// Returns the task lifecycle identity owning this declaration.
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    /// Returns the exact structured domain body.
    #[must_use]
    pub const fn body(&self) -> ContentId<CallerDomainBodyArtifact> {
        self.body
    }

    /// Returns separately archived unknown domain facts.
    #[must_use]
    pub fn explicit_unknowns(&self) -> &[ContentId<DomainUnknownArtifact>] {
        &self.explicit_unknowns
    }

    /// Returns caller-side provenance for the declaration.
    #[must_use]
    pub const fn caller_evidence(&self) -> ContentId<CallerDomainEvidenceArtifact> {
        self.caller_evidence
    }
}

impl TryFrom<DeclaredDomainWire> for DeclaredDomainV1 {
    type Error = VerificationContractError;

    fn try_from(wire: DeclaredDomainWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(
            wire.task_id,
            wire.body,
            wire.explicit_unknowns,
            wire.caller_evidence,
        )
    }
}

/// Proposed domain difference with its own authorship and evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "DomainRefinementWire")]
pub struct DomainRefinementV1 {
    schema_version: VerificationSchemaV1,
    declared_domain: ContentId<DeclaredDomainArtifact>,
    difference: ContentId<DomainDifferenceArtifact>,
    evidence: Vec<ContentId<DomainRefinementEvidenceArtifact>>,
    authorship: ArtifactAuthorshipV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DomainRefinementWire {
    schema_version: VerificationSchemaV1,
    declared_domain: ContentId<DeclaredDomainArtifact>,
    difference: ContentId<DomainDifferenceArtifact>,
    evidence: Vec<ContentId<DomainRefinementEvidenceArtifact>>,
    authorship: ArtifactAuthorshipV1,
}

impl DomainRefinementV1 {
    /// Creates an evidence-citing refinement that cannot overwrite its caller declaration.
    ///
    /// # Errors
    ///
    /// Rejects missing, duplicate, or non-canonical evidence identities.
    pub fn new(
        declared_domain: ContentId<DeclaredDomainArtifact>,
        difference: ContentId<DomainDifferenceArtifact>,
        evidence: Vec<ContentId<DomainRefinementEvidenceArtifact>>,
        authorship: ArtifactAuthorshipV1,
    ) -> Result<Self, VerificationContractError> {
        validate_nonempty_content_ids(&evidence, "domain refinement evidence")?;
        Ok(Self {
            schema_version: VerificationSchemaV1,
            declared_domain,
            difference,
            evidence,
            authorship,
        })
    }

    /// Returns the unmodified caller declaration this proposal refines.
    #[must_use]
    pub const fn declared_domain(&self) -> ContentId<DeclaredDomainArtifact> {
        self.declared_domain
    }

    /// Returns the exact proposed difference.
    #[must_use]
    pub const fn difference(&self) -> ContentId<DomainDifferenceArtifact> {
        self.difference
    }

    /// Returns evidence cited for the proposed difference.
    #[must_use]
    pub fn evidence(&self) -> &[ContentId<DomainRefinementEvidenceArtifact>] {
        &self.evidence
    }

    /// Returns authorship provenance without changing trust.
    #[must_use]
    pub const fn authorship(&self) -> &ArtifactAuthorshipV1 {
        &self.authorship
    }
}

impl TryFrom<DomainRefinementWire> for DomainRefinementV1 {
    type Error = VerificationContractError;

    fn try_from(wire: DomainRefinementWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(
            wire.declared_domain,
            wire.difference,
            wire.evidence,
            wire.authorship,
        )
    }
}

/// Provenance class of a proposed corpus case. It grants no truth by origin.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CorpusCaseSource {
    /// Direct caller proposal.
    Caller,
    /// Mandatory case derived by trusted code from the declared contract.
    TrustedBaseDerivation,
    /// Blue/oracle-author proposal.
    Blue,
    /// Red/oracle-breaker proposal.
    Red,
    /// Upstream test or framework definition.
    Upstream,
    /// External corpus source.
    External,
    /// Fuzz or adversarial search output.
    GeneratedSearch,
    /// Historical failure obligation converted into an explicit case.
    HistoricalFailure,
}

/// One corpus case and its separate source/license provenance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusCaseEntryV1 {
    case: ContentId<CorpusCaseArtifact>,
    source: CorpusCaseSource,
    provenance: ContentId<CorpusCaseProvenanceArtifact>,
    license_provenance: ContentId<LicenseProvenanceArtifact>,
}

impl CorpusCaseEntryV1 {
    /// Creates one proposed case. Source is recorded but never interpreted as truth.
    #[must_use]
    pub const fn new(
        case: ContentId<CorpusCaseArtifact>,
        source: CorpusCaseSource,
        provenance: ContentId<CorpusCaseProvenanceArtifact>,
        license_provenance: ContentId<LicenseProvenanceArtifact>,
    ) -> Self {
        Self {
            case,
            source,
            provenance,
            license_provenance,
        }
    }

    /// Returns the immutable case identity.
    #[must_use]
    pub const fn case(&self) -> ContentId<CorpusCaseArtifact> {
        self.case
    }

    /// Returns case origin without assigning trust.
    #[must_use]
    pub const fn source(&self) -> CorpusCaseSource {
        self.source
    }

    /// Returns exact source provenance.
    #[must_use]
    pub const fn provenance(&self) -> ContentId<CorpusCaseProvenanceArtifact> {
        self.provenance
    }

    /// Returns exact license/provenance material, including an explicit unknown when applicable.
    #[must_use]
    pub const fn license_provenance(&self) -> ContentId<LicenseProvenanceArtifact> {
        self.license_provenance
    }
}

/// Constructor input for a corpus proposal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorpusProposalInput {
    /// Original caller declaration.
    pub declared_domain: ContentId<DeclaredDomainArtifact>,
    /// Separate proposed refinements in strict identity order.
    pub refinements: Vec<ContentId<DomainRefinementArtifact>>,
    /// Proposed cases in strict case-identity order.
    pub cases: Vec<CorpusCaseEntryV1>,
    /// Required domain/historical obligations in strict identity order.
    pub coverage_obligations: Vec<ContentId<CoverageObligationArtifact>>,
}

/// Corpus proposal preserving case origins rather than treating proposal source as truth.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "CorpusProposalWire")]
pub struct CorpusProposalV1 {
    schema_version: VerificationSchemaV1,
    declared_domain: ContentId<DeclaredDomainArtifact>,
    refinements: Vec<ContentId<DomainRefinementArtifact>>,
    cases: Vec<CorpusCaseEntryV1>,
    coverage_obligations: Vec<ContentId<CoverageObligationArtifact>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusProposalWire {
    schema_version: VerificationSchemaV1,
    declared_domain: ContentId<DeclaredDomainArtifact>,
    refinements: Vec<ContentId<DomainRefinementArtifact>>,
    cases: Vec<CorpusCaseEntryV1>,
    coverage_obligations: Vec<ContentId<CoverageObligationArtifact>>,
}

impl CorpusProposalV1 {
    /// Creates a corpus proposal with complete provenance and coverage inputs.
    ///
    /// # Errors
    ///
    /// Rejects empty cases/obligations and all duplicate or non-canonical set inputs.
    pub fn new(input: CorpusProposalInput) -> Result<Self, VerificationContractError> {
        validate_content_id_order(&input.refinements, "corpus domain refinements")?;
        if input.cases.is_empty() {
            return Err(VerificationContractError::EmptySet {
                field: "corpus cases",
            });
        }
        if input
            .cases
            .windows(2)
            .any(|pair| pair[0].case().to_wire() >= pair[1].case().to_wire())
        {
            return Err(VerificationContractError::NonCanonicalSet {
                field: "corpus cases",
            });
        }
        validate_nonempty_content_ids(&input.coverage_obligations, "coverage obligations")?;
        Ok(Self {
            schema_version: VerificationSchemaV1,
            declared_domain: input.declared_domain,
            refinements: input.refinements,
            cases: input.cases,
            coverage_obligations: input.coverage_obligations,
        })
    }

    /// Returns the caller declaration retained by the proposal.
    #[must_use]
    pub const fn declared_domain(&self) -> ContentId<DeclaredDomainArtifact> {
        self.declared_domain
    }

    /// Returns separate refinement identities.
    #[must_use]
    pub fn refinements(&self) -> &[ContentId<DomainRefinementArtifact>] {
        &self.refinements
    }

    /// Returns every proposed case with independent provenance.
    #[must_use]
    pub fn cases(&self) -> &[CorpusCaseEntryV1] {
        &self.cases
    }

    /// Returns required coverage obligations.
    #[must_use]
    pub fn coverage_obligations(&self) -> &[ContentId<CoverageObligationArtifact>] {
        &self.coverage_obligations
    }
}

impl TryFrom<CorpusProposalWire> for CorpusProposalV1 {
    type Error = VerificationContractError;

    fn try_from(wire: CorpusProposalWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(CorpusProposalInput {
            declared_domain: wire.declared_domain,
            refinements: wire.refinements,
            cases: wire.cases,
            coverage_obligations: wire.coverage_obligations,
        })
    }
}

/// Basis for a correct-by-construction claim. It deliberately has no oracle-under-test option.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConstructionJustification {
    /// Structural transformation argument.
    StructuralArgument,
    /// Reference derived independently of the oracle under test.
    IndependentReference,
    /// Exhaustive equivalence over a declared finite domain.
    ExhaustiveFiniteEquivalence,
}

/// Constructor input for one correct-by-construction claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConstructionClaimInput {
    /// Policy-visible construction class.
    pub construction_class: ConstructionClassName,
    /// Exact named transformation.
    pub transformation: TransformationKindName,
    /// Source implementation to which the transformation applies.
    pub source_implementation: ContentId<ImplementationBundleArtifact>,
    /// Explicit prerequisites in strict identity order; may be empty.
    pub prerequisites: Vec<ContentId<ConstructionPrerequisiteArtifact>>,
    /// Independent argument/evidence in strict identity order.
    pub evidence: Vec<ContentId<ConstructionEvidenceArtifact>>,
    /// Recorded justification category.
    pub justification: ConstructionJustification,
    /// Authorship provenance without trust promotion.
    pub authorship: ArtifactAuthorshipV1,
}

/// Correct-by-construction claim whose schema cannot cite passing the oracle under test.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "ConstructionClaimWire")]
pub struct ConstructionClaimV1 {
    schema_version: VerificationSchemaV1,
    construction_class: ConstructionClassName,
    transformation: TransformationKindName,
    source_implementation: ContentId<ImplementationBundleArtifact>,
    prerequisites: Vec<ContentId<ConstructionPrerequisiteArtifact>>,
    evidence: Vec<ContentId<ConstructionEvidenceArtifact>>,
    justification: ConstructionJustification,
    authorship: ArtifactAuthorshipV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConstructionClaimWire {
    schema_version: VerificationSchemaV1,
    construction_class: ConstructionClassName,
    transformation: TransformationKindName,
    source_implementation: ContentId<ImplementationBundleArtifact>,
    prerequisites: Vec<ContentId<ConstructionPrerequisiteArtifact>>,
    evidence: Vec<ContentId<ConstructionEvidenceArtifact>>,
    justification: ConstructionJustification,
    authorship: ArtifactAuthorshipV1,
}

impl ConstructionClaimV1 {
    /// Creates a construction claim with evidence independent in shape from the oracle under test.
    ///
    /// # Errors
    ///
    /// Rejects non-canonical prerequisites and missing/non-canonical evidence.
    pub fn new(input: ConstructionClaimInput) -> Result<Self, VerificationContractError> {
        validate_content_id_order(&input.prerequisites, "construction prerequisites")?;
        validate_nonempty_content_ids(&input.evidence, "construction evidence")?;
        Ok(Self {
            schema_version: VerificationSchemaV1,
            construction_class: input.construction_class,
            transformation: input.transformation,
            source_implementation: input.source_implementation,
            prerequisites: input.prerequisites,
            evidence: input.evidence,
            justification: input.justification,
            authorship: input.authorship,
        })
    }

    /// Returns the policy-visible construction class.
    #[must_use]
    pub const fn construction_class(&self) -> &ConstructionClassName {
        &self.construction_class
    }

    /// Returns the exact source implementation transformed.
    #[must_use]
    pub const fn source_implementation(&self) -> ContentId<ImplementationBundleArtifact> {
        self.source_implementation
    }

    /// Returns independent evidence identities.
    #[must_use]
    pub fn evidence(&self) -> &[ContentId<ConstructionEvidenceArtifact>] {
        &self.evidence
    }
}

impl TryFrom<ConstructionClaimWire> for ConstructionClaimV1 {
    type Error = VerificationContractError;

    fn try_from(wire: ConstructionClaimWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(ConstructionClaimInput {
            construction_class: wire.construction_class,
            transformation: wire.transformation,
            source_implementation: wire.source_implementation,
            prerequisites: wire.prerequisites,
            evidence: wire.evidence,
            justification: wire.justification,
            authorship: wire.authorship,
        })
    }
}

/// Admission behavior a variant is constructed to require.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "required_response",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum VariantExpectation {
    /// A false-reject control backed by an independent construction claim.
    MustAccept {
        /// Exact construction claim that makes the variant a required honest path.
        construction_claim: ContentId<ConstructionClaimArtifact>,
    },
    /// A false-accept control backed by a named fault and evidence of injection.
    MustReject {
        /// Policy-visible fault class.
        fault_class: FaultClassName,
        /// Exact evidence showing the fault was introduced into the implementation.
        fault_evidence: ContentId<FaultInjectionEvidenceArtifact>,
    },
}

/// Correct or deliberately incorrect implementation variant before execution.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImplementationVariantV1 {
    schema_version: VerificationSchemaV1,
    implementation: ContentId<ImplementationBundleArtifact>,
    expectation: VariantExpectation,
    authorship: ArtifactAuthorshipV1,
}

impl ImplementationVariantV1 {
    /// Creates a variant whose required behavior is encoded by a closed typed alternative.
    #[must_use]
    pub const fn new(
        implementation: ContentId<ImplementationBundleArtifact>,
        expectation: VariantExpectation,
        authorship: ArtifactAuthorshipV1,
    ) -> Self {
        Self {
            schema_version: VerificationSchemaV1,
            implementation,
            expectation,
            authorship,
        }
    }

    /// Returns the exact implementation bundle.
    #[must_use]
    pub const fn implementation(&self) -> ContentId<ImplementationBundleArtifact> {
        self.implementation
    }

    /// Returns whether admission must accept or reject this variant and why.
    #[must_use]
    pub const fn expectation(&self) -> &VariantExpectation {
        &self.expectation
    }
}

/// Constructor input for an immutable oracle proposal bundle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleProposalInput {
    /// Stable task lifecycle identity.
    pub task_id: TaskId,
    /// Frozen oracle-visible task inputs.
    pub task_inputs: ContentId<OracleTaskInputArtifact>,
    /// Original caller declaration.
    pub declared_domain: ContentId<DeclaredDomainArtifact>,
    /// Separate proposed domain refinements in strict identity order.
    pub domain_refinements: Vec<ContentId<DomainRefinementArtifact>>,
    /// Proposed corpus and derivation provenance.
    pub corpus_proposal: ContentId<CorpusProposalArtifact>,
    /// Proposed references/allowed-result sets in strict identity order.
    pub references: Vec<ContentId<ReferenceArtifact>>,
    /// Proposed properties/metamorphic relations in strict identity order.
    pub properties: Vec<ContentId<PropertyRelationArtifact>>,
    /// Proposed source interrogation plan.
    pub source_admission_plan: ContentId<SourceAdmissionPlanArtifact>,
    /// Proposed valid-family generation plan.
    pub valid_family_plan: ContentId<ValidFamilyPlanArtifact>,
    /// Proposed observation ABI/result schema.
    pub observation_plan: ContentId<ObservationPlanArtifact>,
    /// Strength requested from admission.
    pub requested_strength: OracleStrength,
    /// Proposal authorship/model/configuration provenance.
    pub authorship: ArtifactAuthorshipV1,
}

/// Oracle proposal bundle with no trusted policy, allowance, mutants, comparison, or decision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "OracleProposalWire")]
pub struct OracleProposalV1 {
    schema_version: VerificationSchemaV1,
    task_id: TaskId,
    task_inputs: ContentId<OracleTaskInputArtifact>,
    declared_domain: ContentId<DeclaredDomainArtifact>,
    domain_refinements: Vec<ContentId<DomainRefinementArtifact>>,
    corpus_proposal: ContentId<CorpusProposalArtifact>,
    references: Vec<ContentId<ReferenceArtifact>>,
    properties: Vec<ContentId<PropertyRelationArtifact>>,
    source_admission_plan: ContentId<SourceAdmissionPlanArtifact>,
    valid_family_plan: ContentId<ValidFamilyPlanArtifact>,
    observation_plan: ContentId<ObservationPlanArtifact>,
    requested_strength: OracleStrength,
    authorship: ArtifactAuthorshipV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OracleProposalWire {
    schema_version: VerificationSchemaV1,
    task_id: TaskId,
    task_inputs: ContentId<OracleTaskInputArtifact>,
    declared_domain: ContentId<DeclaredDomainArtifact>,
    domain_refinements: Vec<ContentId<DomainRefinementArtifact>>,
    corpus_proposal: ContentId<CorpusProposalArtifact>,
    references: Vec<ContentId<ReferenceArtifact>>,
    properties: Vec<ContentId<PropertyRelationArtifact>>,
    source_admission_plan: ContentId<SourceAdmissionPlanArtifact>,
    valid_family_plan: ContentId<ValidFamilyPlanArtifact>,
    observation_plan: ContentId<ObservationPlanArtifact>,
    requested_strength: OracleStrength,
    authorship: ArtifactAuthorshipV1,
}

impl OracleProposalV1 {
    /// Creates a proposal without granting it admission authority.
    ///
    /// # Errors
    ///
    /// Rejects non-canonical collections, unavailable as a requested strength, reference strength
    /// without a reference, and property strength without a property relation.
    pub fn new(input: OracleProposalInput) -> Result<Self, VerificationContractError> {
        validate_content_id_order(&input.domain_refinements, "proposal domain refinements")?;
        validate_content_id_order(&input.references, "proposal references")?;
        validate_content_id_order(&input.properties, "proposal properties")?;
        match input.requested_strength {
            OracleStrength::Reference if input.references.is_empty() => {
                return invalid(
                    "oracle proposal",
                    "reference strength requires at least one proposed reference",
                );
            }
            OracleStrength::PropertyMetamorphic if input.properties.is_empty() => {
                return invalid(
                    "oracle proposal",
                    "property/metamorphic strength requires at least one proposed relation",
                );
            }
            OracleStrength::Unavailable => {
                return invalid(
                    "oracle proposal",
                    "unavailable is an admission outcome, not a requested strength",
                );
            }
            OracleStrength::Reference
            | OracleStrength::PropertyMetamorphic
            | OracleStrength::Implicit => {}
        }
        Ok(Self {
            schema_version: VerificationSchemaV1,
            task_id: input.task_id,
            task_inputs: input.task_inputs,
            declared_domain: input.declared_domain,
            domain_refinements: input.domain_refinements,
            corpus_proposal: input.corpus_proposal,
            references: input.references,
            properties: input.properties,
            source_admission_plan: input.source_admission_plan,
            valid_family_plan: input.valid_family_plan,
            observation_plan: input.observation_plan,
            requested_strength: input.requested_strength,
            authorship: input.authorship,
        })
    }

    /// Returns the stable task lifecycle identity.
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    /// Returns the unmodified caller declaration.
    #[must_use]
    pub const fn declared_domain(&self) -> ContentId<DeclaredDomainArtifact> {
        self.declared_domain
    }

    /// Returns the proposed corpus as an immutable, separately validated artifact.
    #[must_use]
    pub const fn corpus_proposal(&self) -> ContentId<CorpusProposalArtifact> {
        self.corpus_proposal
    }

    /// Returns proposed semantic references in canonical identity order.
    #[must_use]
    pub fn references(&self) -> &[ContentId<ReferenceArtifact>] {
        &self.references
    }

    /// Returns proposed refinements as separate artifacts.
    #[must_use]
    pub fn domain_refinements(&self) -> &[ContentId<DomainRefinementArtifact>] {
        &self.domain_refinements
    }

    /// Returns requested strength; this is not an admission outcome.
    #[must_use]
    pub const fn requested_strength(&self) -> OracleStrength {
        self.requested_strength
    }
}

impl TryFrom<OracleProposalWire> for OracleProposalV1 {
    type Error = VerificationContractError;

    fn try_from(wire: OracleProposalWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(OracleProposalInput {
            task_id: wire.task_id,
            task_inputs: wire.task_inputs,
            declared_domain: wire.declared_domain,
            domain_refinements: wire.domain_refinements,
            corpus_proposal: wire.corpus_proposal,
            references: wire.references,
            properties: wire.properties,
            source_admission_plan: wire.source_admission_plan,
            valid_family_plan: wire.valid_family_plan,
            observation_plan: wire.observation_plan,
            requested_strength: wire.requested_strength,
            authorship: wire.authorship,
        })
    }
}

fn validate_nonempty_content_ids<T: ContentType>(
    values: &[ContentId<T>],
    field: &'static str,
) -> Result<(), VerificationContractError> {
    if values.is_empty() {
        return Err(VerificationContractError::EmptySet { field });
    }
    validate_content_id_order(values, field)
}

fn invalid<T>(
    artifact: &'static str,
    reason: &'static str,
) -> Result<T, VerificationContractError> {
    Err(VerificationContractError::InvalidArtifactCombination { artifact, reason })
}

#[cfg(test)]
mod tests {
    use cairn_protocol::{ContentId, ContentType, EpisodeId, TaskId};

    use super::{
        ArtifactAuthorId, ArtifactAuthorshipV1, AuthorshipOrigin, CallerDomainBodyArtifact,
        CallerDomainEvidenceArtifact, ConstructionClaimArtifact, ConstructionClaimInput,
        ConstructionClaimV1, ConstructionClassName, ConstructionEvidenceArtifact,
        ConstructionJustification, ConstructionPrerequisiteArtifact, CorpusCaseArtifact,
        CorpusCaseEntryV1, CorpusCaseProvenanceArtifact, CorpusCaseSource, CorpusProposalArtifact,
        CorpusProposalInput, CorpusProposalV1, CoverageObligationArtifact, DeclaredDomainArtifact,
        DeclaredDomainV1, DomainDifferenceArtifact, DomainRefinementArtifact,
        DomainRefinementEvidenceArtifact, DomainRefinementV1, DomainUnknownArtifact,
        FaultClassName, FaultInjectionEvidenceArtifact, ImplementationBundleArtifact,
        ImplementationVariantV1, LicenseProvenanceArtifact, ModelConfigurationArtifact,
        ObservationPlanArtifact, OracleProposalArtifact, OracleProposalInput, OracleProposalV1,
        OracleStrength, OracleTaskInputArtifact, PropertyRelationArtifact, ReferenceArtifact,
        SourceAdmissionPlanArtifact, TransformationKindName, ValidFamilyPlanArtifact,
        VariantExpectation, VerificationContractError,
    };

    fn id<T: ContentType>(seed: &str) -> ContentId<T> {
        ContentId::derive(seed.as_bytes()).expect("content identity")
    }

    fn repository_authorship() -> ArtifactAuthorshipV1 {
        ArtifactAuthorshipV1::new(
            AuthorshipOrigin::Repository,
            ArtifactAuthorId::new("cairn-testkit").expect("author"),
            None,
            None,
        )
        .expect("authorship")
    }

    fn declared_domain() -> DeclaredDomainV1 {
        DeclaredDomainV1::new(
            TaskId::new(),
            id::<CallerDomainBodyArtifact>("domain-body"),
            vec![id::<DomainUnknownArtifact>("unknown-alignment")],
            id::<CallerDomainEvidenceArtifact>("caller-evidence"),
        )
        .expect("declared domain")
    }

    fn declared_domain_id() -> ContentId<DeclaredDomainArtifact> {
        let bytes = cairn_codec::to_vec(&declared_domain()).expect("domain bytes");
        ContentId::derive(&bytes).expect("domain identity")
    }

    fn corpus_proposal() -> CorpusProposalV1 {
        CorpusProposalV1::new(CorpusProposalInput {
            declared_domain: declared_domain_id(),
            refinements: Vec::new(),
            cases: vec![CorpusCaseEntryV1::new(
                id::<CorpusCaseArtifact>("base-case"),
                CorpusCaseSource::TrustedBaseDerivation,
                id::<CorpusCaseProvenanceArtifact>("case-provenance"),
                id::<LicenseProvenanceArtifact>("project-authored-license"),
            )],
            coverage_obligations: vec![id::<CoverageObligationArtifact>("zero-length")],
        })
        .expect("corpus")
    }

    fn proposal_input(strength: OracleStrength) -> OracleProposalInput {
        let corpus_bytes = cairn_codec::to_vec(&corpus_proposal()).expect("corpus bytes");
        OracleProposalInput {
            task_id: TaskId::new(),
            task_inputs: id::<OracleTaskInputArtifact>("task-inputs"),
            declared_domain: declared_domain_id(),
            domain_refinements: Vec::new(),
            corpus_proposal: ContentId::<CorpusProposalArtifact>::derive(&corpus_bytes)
                .expect("corpus identity"),
            references: vec![id::<ReferenceArtifact>("high-precision-reference")],
            properties: vec![id::<PropertyRelationArtifact>("linearity-property")],
            source_admission_plan: id::<SourceAdmissionPlanArtifact>("source-plan"),
            valid_family_plan: id::<ValidFamilyPlanArtifact>("family-plan"),
            observation_plan: id::<ObservationPlanArtifact>("observation-plan"),
            requested_strength: strength,
            authorship: repository_authorship(),
        }
    }

    #[test]
    fn model_authorship_requires_exact_episode_and_configuration_provenance() {
        let author = ArtifactAuthorId::new("blue-model").expect("author");
        assert!(matches!(
            ArtifactAuthorshipV1::new(AuthorshipOrigin::Model, author.clone(), None, None),
            Err(VerificationContractError::InvalidArtifactCombination { .. })
        ));
        let authorship = ArtifactAuthorshipV1::new(
            AuthorshipOrigin::Model,
            author,
            Some(EpisodeId::new()),
            Some(id::<ModelConfigurationArtifact>("resolved-model")),
        )
        .expect("model authorship");
        let bytes = cairn_codec::to_vec(&authorship).expect("authorship bytes");
        assert_eq!(
            cairn_codec::from_slice::<ArtifactAuthorshipV1>(&bytes).expect("strict authorship"),
            authorship
        );
        assert!(
            ArtifactAuthorshipV1::new(
                AuthorshipOrigin::Human,
                ArtifactAuthorId::new("reviewer").expect("author"),
                None,
                Some(id::<ModelConfigurationArtifact>("hidden-model")),
            )
            .is_err()
        );
    }

    #[test]
    fn caller_domain_keeps_explicit_unknowns_and_changes_identity_with_every_input() {
        let domain = declared_domain();
        let bytes = cairn_codec::to_vec(&domain).expect("domain bytes");
        assert_eq!(
            cairn_codec::from_slice::<DeclaredDomainV1>(&bytes).expect("strict domain"),
            domain
        );
        let original = ContentId::<DeclaredDomainArtifact>::derive(&bytes).expect("identity");
        let changed = DeclaredDomainV1::new(
            domain.task_id(),
            id::<CallerDomainBodyArtifact>("changed-body"),
            domain.explicit_unknowns().to_vec(),
            domain.caller_evidence(),
        )
        .expect("changed domain");
        let changed_bytes = cairn_codec::to_vec(&changed).expect("changed bytes");
        assert_ne!(
            original,
            ContentId::<DeclaredDomainArtifact>::derive(&changed_bytes).expect("changed identity")
        );

        let unknown_a = id::<DomainUnknownArtifact>("a");
        let unknown_b = id::<DomainUnknownArtifact>("b");
        let mut unknowns = vec![unknown_a, unknown_b];
        unknowns.sort_by_key(ContentId::to_wire);
        unknowns.reverse();
        assert!(
            DeclaredDomainV1::new(
                TaskId::new(),
                id::<CallerDomainBodyArtifact>("body"),
                unknowns,
                id::<CallerDomainEvidenceArtifact>("evidence"),
            )
            .is_err()
        );
    }

    #[test]
    fn refinements_and_corpus_preserve_sources_and_fail_closed_on_missing_provenance() {
        assert!(
            DomainRefinementV1::new(
                declared_domain_id(),
                id::<DomainDifferenceArtifact>("shape-refinement"),
                Vec::new(),
                repository_authorship(),
            )
            .is_err()
        );
        let refinement = DomainRefinementV1::new(
            declared_domain_id(),
            id::<DomainDifferenceArtifact>("shape-refinement"),
            vec![id::<DomainRefinementEvidenceArtifact>(
                "upstream-definition",
            )],
            repository_authorship(),
        )
        .expect("refinement");
        let refinement_bytes = cairn_codec::to_vec(&refinement).expect("refinement bytes");
        let refinement_id = ContentId::<DomainRefinementArtifact>::derive(&refinement_bytes)
            .expect("refinement identity");

        let case_a = CorpusCaseEntryV1::new(
            id::<CorpusCaseArtifact>("a"),
            CorpusCaseSource::Caller,
            id::<CorpusCaseProvenanceArtifact>("caller-case"),
            id::<LicenseProvenanceArtifact>("caller-license"),
        );
        let case_b = CorpusCaseEntryV1::new(
            id::<CorpusCaseArtifact>("b"),
            CorpusCaseSource::External,
            id::<CorpusCaseProvenanceArtifact>("external-case"),
            id::<LicenseProvenanceArtifact>("external-license"),
        );
        let mut cases = vec![case_a, case_b];
        cases.sort_by_key(|entry| entry.case().to_wire());
        let corpus = CorpusProposalV1::new(CorpusProposalInput {
            declared_domain: declared_domain_id(),
            refinements: vec![refinement_id],
            cases: cases.clone(),
            coverage_obligations: vec![id::<CoverageObligationArtifact>("boundary")],
        })
        .expect("corpus");
        assert!(corpus.cases().iter().any(|entry| {
            matches!(entry.source(), CorpusCaseSource::External)
                && entry.license_provenance() == id::<LicenseProvenanceArtifact>("external-license")
        }));
        cases.reverse();
        assert!(
            CorpusProposalV1::new(CorpusProposalInput {
                declared_domain: declared_domain_id(),
                refinements: vec![refinement_id],
                cases,
                coverage_obligations: vec![id::<CoverageObligationArtifact>("boundary")],
            })
            .is_err()
        );
    }

    #[test]
    fn construction_claim_and_variant_shape_cannot_self_grade() {
        assert!(
            ConstructionClaimV1::new(ConstructionClaimInput {
                construction_class: ConstructionClassName::new("tree-order").expect("class"),
                transformation: TransformationKindName::new("balanced-tree").expect("kind"),
                source_implementation: id::<ImplementationBundleArtifact>("source"),
                prerequisites: Vec::new(),
                evidence: Vec::new(),
                justification: ConstructionJustification::StructuralArgument,
                authorship: repository_authorship(),
            })
            .is_err()
        );
        let claim = ConstructionClaimV1::new(ConstructionClaimInput {
            construction_class: ConstructionClassName::new("tree-order").expect("class"),
            transformation: TransformationKindName::new("balanced-tree").expect("kind"),
            source_implementation: id::<ImplementationBundleArtifact>("source"),
            prerequisites: vec![id::<ConstructionPrerequisiteArtifact>(
                "associative-contract",
            )],
            evidence: vec![id::<ConstructionEvidenceArtifact>("structural-argument")],
            justification: ConstructionJustification::StructuralArgument,
            authorship: repository_authorship(),
        })
        .expect("claim");
        let claim_bytes = cairn_codec::to_vec(&claim).expect("claim bytes");
        let accepted = ImplementationVariantV1::new(
            id::<ImplementationBundleArtifact>("tree-variant"),
            VariantExpectation::MustAccept {
                construction_claim: ContentId::<ConstructionClaimArtifact>::derive(&claim_bytes)
                    .expect("claim identity"),
            },
            repository_authorship(),
        );
        let wrong = ImplementationVariantV1::new(
            id::<ImplementationBundleArtifact>("offset-variant"),
            VariantExpectation::MustReject {
                fault_class: FaultClassName::new("arithmetic-offset").expect("fault"),
                fault_evidence: id::<FaultInjectionEvidenceArtifact>("offset-patch"),
            },
            repository_authorship(),
        );
        assert!(matches!(
            accepted.expectation(),
            VariantExpectation::MustAccept { .. }
        ));
        assert!(matches!(
            wrong.expectation(),
            VariantExpectation::MustReject { .. }
        ));

        let mut value = serde_json::to_value(&accepted).expect("variant json");
        value["expectation"]["fault_class"] = serde_json::json!("smuggled-fault");
        assert!(serde_json::from_value::<ImplementationVariantV1>(value).is_err());
    }

    #[test]
    fn proposal_enforces_strength_inputs_and_rejects_trusted_adjudication_fields() {
        let proposal = OracleProposalV1::new(proposal_input(OracleStrength::Reference))
            .expect("reference proposal");
        let bytes = cairn_codec::to_vec(&proposal).expect("proposal bytes");
        assert_eq!(
            cairn_codec::from_slice::<OracleProposalV1>(&bytes).expect("strict proposal"),
            proposal
        );
        assert_eq!(proposal.requested_strength(), OracleStrength::Reference);

        let mut missing_reference = proposal_input(OracleStrength::Reference);
        missing_reference.references.clear();
        assert!(OracleProposalV1::new(missing_reference).is_err());
        assert!(OracleProposalV1::new(proposal_input(OracleStrength::Unavailable)).is_err());

        for forbidden in [
            "admission_policy",
            "numerical_allowance",
            "trusted_mutants",
            "comparison_policy",
            "admission_decision",
        ] {
            let mut value: serde_json::Value =
                serde_json::from_slice(&bytes).expect("proposal json");
            value[forbidden] = serde_json::json!("smuggled");
            assert!(
                serde_json::from_value::<OracleProposalV1>(value).is_err(),
                "accepted forbidden field {forbidden}"
            );
        }
    }

    #[test]
    fn proposal_identity_commits_to_every_verdict_relevant_edge() {
        let proposal =
            OracleProposalV1::new(proposal_input(OracleStrength::Reference)).expect("proposal");
        let base_value = serde_json::to_value(&proposal).expect("proposal json");
        let base_bytes = cairn_codec::to_vec(&base_value).expect("base bytes");
        let base_id = ContentId::<OracleProposalArtifact>::derive(&base_bytes).expect("base id");

        let replacements = [
            (
                "task_inputs",
                id::<OracleTaskInputArtifact>("changed").to_wire(),
            ),
            (
                "declared_domain",
                id::<DeclaredDomainArtifact>("changed").to_wire(),
            ),
            (
                "corpus_proposal",
                id::<CorpusProposalArtifact>("changed").to_wire(),
            ),
            (
                "source_admission_plan",
                id::<SourceAdmissionPlanArtifact>("changed").to_wire(),
            ),
            (
                "valid_family_plan",
                id::<ValidFamilyPlanArtifact>("changed").to_wire(),
            ),
            (
                "observation_plan",
                id::<ObservationPlanArtifact>("changed").to_wire(),
            ),
        ];
        for (field, replacement) in replacements {
            let mut changed = base_value.clone();
            changed[field] = serde_json::Value::String(replacement);
            let bytes = cairn_codec::to_vec(&changed).expect("changed bytes");
            assert_ne!(
                ContentId::<OracleProposalArtifact>::derive(&bytes).expect("changed id"),
                base_id,
                "identity ignored {field}"
            );
        }

        let mut changed_reference = base_value.clone();
        changed_reference["references"] =
            serde_json::json!([id::<ReferenceArtifact>("different-reference").to_wire()]);
        let bytes = cairn_codec::to_vec(&changed_reference).expect("changed reference bytes");
        assert_ne!(
            ContentId::<OracleProposalArtifact>::derive(&bytes).expect("changed reference id"),
            base_id
        );
    }

    #[test]
    fn all_new_artifacts_reject_non_v1_and_unknown_fields() {
        let bytes = cairn_codec::to_vec(&declared_domain()).expect("domain bytes");
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("domain json");
        value["schema_version"] = serde_json::json!(2);
        assert!(serde_json::from_value::<DeclaredDomainV1>(value.clone()).is_err());
        value["schema_version"] = serde_json::json!(1);
        value["legacy_domain"] = serde_json::json!(true);
        assert!(serde_json::from_value::<DeclaredDomainV1>(value).is_err());

        assert_ne!(
            DeclaredDomainArtifact::DOMAIN,
            OracleProposalArtifact::DOMAIN
        );
        assert_ne!(
            ImplementationBundleArtifact::DOMAIN,
            ConstructionClaimArtifact::DOMAIN
        );
    }
}
