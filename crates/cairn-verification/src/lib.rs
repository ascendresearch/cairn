//! Domain-neutral trusted verification contracts.
//!
//! This crate owns admission and verdict policy, never operator mathematics, model transport, or
//! execution. Current slices establish immutable V1 proposal, policy, numerical allowance,
//! admitted-oracle, and candidate-verdict boundaries. Product crates supply and recompute the
//! execution and comparison evidence consumed by these contracts.

use std::{fmt, str::FromStr};

use cairn_protocol::{ContentId, ContentType};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

const MAX_POLICY_LABEL_LEN: usize = 128;
const MAX_DECIMAL_LEN: usize = 128;

/// Failure to construct a trustworthy verification contract.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum VerificationContractError {
    /// Only the current pre-release V1 contract is accepted.
    #[error("verification schema version must be 1")]
    UnsupportedSchemaVersion,
    /// A policy label is empty, too long, or outside the conservative wire alphabet.
    #[error("{kind} is not a valid verification policy label")]
    InvalidLabel {
        /// Semantic label kind that failed validation.
        kind: &'static str,
    },
    /// A configured positive quantity used zero.
    #[error("{field} must be greater than zero")]
    NonPositive {
        /// Quantity that failed validation.
        field: &'static str,
    },
    /// A set-like field was empty.
    #[error("{field} must contain at least one value")]
    EmptySet {
        /// Collection that failed validation.
        field: &'static str,
    },
    /// A canonical set was duplicated or out of order.
    #[error("{field} must be in strict canonical order without duplicates")]
    NonCanonicalSet {
        /// Collection that failed validation.
        field: &'static str,
    },
    /// A numerical allowance had neither an absolute nor relative bound.
    #[error("a numerical allowance requires an absolute or relative magnitude")]
    MissingMagnitude,
    /// An allowance magnitude was not a canonical non-negative decimal.
    #[error("allowance magnitude must be a canonical non-negative decimal")]
    InvalidMagnitude,
    /// Measured evidence did not cite the corpus from which it was derived.
    #[error("{evidence} evidence requires a non-empty derivation corpus")]
    MissingDerivationCorpus {
        /// Evidence class requiring derivation inputs.
        evidence: &'static str,
    },
    /// Held-out evidence did not cite an independent validation corpus.
    #[error("held-out validation requires a non-empty validation corpus")]
    MissingValidationCorpus,
    /// The same corpus was used for derivation and held-out validation.
    #[error("derivation and held-out validation corpora must be identity-disjoint")]
    CorpusOverlap,
    /// Artifact fields represented a contradictory semantic shape.
    #[error("{artifact} is invalid: {reason}")]
    InvalidArtifactCombination {
        /// Artifact whose invariant failed.
        artifact: &'static str,
        /// Stable diagnostic for the failed invariant.
        reason: &'static str,
    },
}

/// The only verification schema version accepted during pre-release development.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VerificationSchemaV1;

impl Serialize for VerificationSchemaV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(1)
    }
}

impl<'de> Deserialize<'de> for VerificationSchemaV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match u32::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(de::Error::custom(
                VerificationContractError::UnsupportedSchemaVersion,
            )),
        }
    }
}

fn validate_label(value: &str, kind: &'static str) -> Result<(), VerificationContractError> {
    if value.is_empty()
        || value.len() > MAX_POLICY_LABEL_LEN
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'.' | b'_' | b'/')
        })
    {
        return Err(VerificationContractError::InvalidLabel { kind });
    }
    Ok(())
}

macro_rules! policy_label {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a validated policy label.
            ///
            /// # Errors
            ///
            /// Rejects an empty, oversized, or non-canonical label.
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

policy_label!(
    /// A structurally meaningful correct-variant construction class.
    ConstructionClassName,
    "construction class"
);
policy_label!(
    /// A deliberately incorrect implementation or generic mutant fault class.
    FaultClassName,
    "fault class"
);
policy_label!(
    /// A justified region over which one numerical allowance applies.
    DomainRegionName,
    "domain region"
);

macro_rules! positive_u32 {
    ($(#[$meta:meta])* $name:ident, $field:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(u32);

        impl $name {
            /// Creates a positive policy quantity.
            ///
            /// # Errors
            ///
            /// Rejects zero rather than assigning it disabled or unbounded meaning.
            pub fn new(value: u32) -> Result<Self, VerificationContractError> {
                if value == 0 {
                    return Err(VerificationContractError::NonPositive { field: $field });
                }
                Ok(Self(value))
            }

            /// Returns the configured positive value.
            #[must_use]
            pub const fn get(self) -> u32 {
                self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

positive_u32!(
    /// Minimum accepted correct-by-construction variant count.
    CorrectVariantMinimum,
    "minimum correct variant count"
);
positive_u32!(
    /// Minimum rejected implementation-level incorrect variant count.
    IncorrectVariantMinimum,
    "minimum incorrect variant count"
);
positive_u32!(
    /// Consecutive mutation-search rounds required without a newly discovered class.
    SaturationRoundCount,
    "saturation round count"
);

/// Strength of semantic claim requested from or admitted for an oracle.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleStrength {
    /// Direct semantic reference or admitted allowed-result set.
    Reference,
    /// Property or metamorphic relation without a direct result reference.
    PropertyMetamorphic,
    /// Invocation, shape, status, or other non-semantic checks only.
    Implicit,
    /// No requested claim strength could be established.
    Unavailable,
}

/// Executed boundary an admission policy requires evidence to traverse.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdmissionExecutionScope {
    /// Trusted comparison code only.
    Comparator,
    /// Observation production and decoding path.
    ObservationPipeline,
    /// Real implementation build and execution path.
    Implementation,
    /// Source-accelerator execution.
    SourceAccelerator,
    /// Target build environment.
    TargetBuild,
    /// Target-device execution.
    TargetDevice,
}

/// Required relationship among correct-by-construction evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructuralIndependenceRequirement {
    /// The selected strength permits no structural-independence claim.
    NotRequired,
    /// Every accepted variant must cite a distinct construction claim.
    DistinctConstructionClaims,
}

/// Result forced when admission spend ends before the selected policy is satisfied.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BudgetExhaustionOutcome {
    /// The current proposal is rejected and may be revised.
    Reject,
    /// Available evidence cannot establish the requested claim.
    Unverifiable,
}

/// Immutable, domain-neutral admission policy artifact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "AdmissionPolicyWire")]
pub struct AdmissionPolicyV1 {
    schema_version: VerificationSchemaV1,
    mutant_set: ContentId<mutation::GenericMutantSetArtifact>,
    minimum_correct_variants: CorrectVariantMinimum,
    minimum_incorrect_variants: IncorrectVariantMinimum,
    required_construction_classes: Vec<ConstructionClassName>,
    required_fault_classes: Vec<FaultClassName>,
    structural_independence: StructuralIndependenceRequirement,
    saturation_rounds: SaturationRoundCount,
    accepted_strengths: Vec<OracleStrength>,
    required_execution_scopes: Vec<AdmissionExecutionScope>,
    budget_exhaustion_outcome: BudgetExhaustionOutcome,
}

/// Constructor input for one immutable admission policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmissionPolicyInput {
    /// Exact trusted generic-mutant definitions and versions selected by this policy.
    pub mutant_set: ContentId<mutation::GenericMutantSetArtifact>,
    /// Minimum correct variants the receipt must prove accepted.
    pub minimum_correct_variants: CorrectVariantMinimum,
    /// Minimum incorrect variants the receipt must prove rejected.
    pub minimum_incorrect_variants: IncorrectVariantMinimum,
    /// Required correct construction classes in strict canonical order.
    pub required_construction_classes: Vec<ConstructionClassName>,
    /// Required incorrect/mutant fault classes in strict canonical order.
    pub required_fault_classes: Vec<FaultClassName>,
    /// Structural-independence rule for correct variants.
    pub structural_independence: StructuralIndependenceRequirement,
    /// Consecutive no-new-class rounds required for saturation.
    pub saturation_rounds: SaturationRoundCount,
    /// Strengths this policy may admit, in strict canonical order.
    pub accepted_strengths: Vec<OracleStrength>,
    /// Evidence scopes required by this policy, in strict canonical order.
    pub required_execution_scopes: Vec<AdmissionExecutionScope>,
    /// Outcome when budget ends before every obligation is satisfied.
    pub budget_exhaustion_outcome: BudgetExhaustionOutcome,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmissionPolicyWire {
    schema_version: VerificationSchemaV1,
    mutant_set: ContentId<mutation::GenericMutantSetArtifact>,
    minimum_correct_variants: CorrectVariantMinimum,
    minimum_incorrect_variants: IncorrectVariantMinimum,
    required_construction_classes: Vec<ConstructionClassName>,
    required_fault_classes: Vec<FaultClassName>,
    structural_independence: StructuralIndependenceRequirement,
    saturation_rounds: SaturationRoundCount,
    accepted_strengths: Vec<OracleStrength>,
    required_execution_scopes: Vec<AdmissionExecutionScope>,
    budget_exhaustion_outcome: BudgetExhaustionOutcome,
}

impl AdmissionPolicyV1 {
    /// Validates a complete V1 admission policy without supplying verifier-global defaults.
    ///
    /// # Errors
    ///
    /// Rejects empty or non-canonical set fields. Counts and stopping behavior remain explicit
    /// policy inputs rather than constants embedded in admission adjudication.
    pub fn new(input: AdmissionPolicyInput) -> Result<Self, VerificationContractError> {
        validate_nonempty_ordered(
            &input.required_construction_classes,
            "required construction classes",
        )?;
        validate_nonempty_ordered(&input.required_fault_classes, "required fault classes")?;
        validate_nonempty_ordered(&input.accepted_strengths, "accepted strengths")?;
        if input
            .accepted_strengths
            .contains(&OracleStrength::Unavailable)
        {
            return Err(VerificationContractError::InvalidLabel {
                kind: "admissible oracle strength",
            });
        }
        validate_nonempty_ordered(
            &input.required_execution_scopes,
            "required execution scopes",
        )?;
        Ok(Self {
            schema_version: VerificationSchemaV1,
            mutant_set: input.mutant_set,
            minimum_correct_variants: input.minimum_correct_variants,
            minimum_incorrect_variants: input.minimum_incorrect_variants,
            required_construction_classes: input.required_construction_classes,
            required_fault_classes: input.required_fault_classes,
            structural_independence: input.structural_independence,
            saturation_rounds: input.saturation_rounds,
            accepted_strengths: input.accepted_strengths,
            required_execution_scopes: input.required_execution_scopes,
            budget_exhaustion_outcome: input.budget_exhaustion_outcome,
        })
    }

    /// Returns the exact trusted generic-mutant set selected by this policy.
    #[must_use]
    pub const fn mutant_set(&self) -> ContentId<mutation::GenericMutantSetArtifact> {
        self.mutant_set
    }

    /// Returns the configured minimum accepted correct-variant count.
    #[must_use]
    pub const fn minimum_correct_variants(&self) -> CorrectVariantMinimum {
        self.minimum_correct_variants
    }

    /// Returns the configured minimum rejected incorrect-variant count.
    #[must_use]
    pub const fn minimum_incorrect_variants(&self) -> IncorrectVariantMinimum {
        self.minimum_incorrect_variants
    }

    /// Returns required construction classes in canonical order.
    #[must_use]
    pub fn required_construction_classes(&self) -> &[ConstructionClassName] {
        &self.required_construction_classes
    }

    /// Returns required fault classes in canonical order.
    #[must_use]
    pub fn required_fault_classes(&self) -> &[FaultClassName] {
        &self.required_fault_classes
    }

    /// Returns the configured structural-independence rule.
    #[must_use]
    pub const fn structural_independence(&self) -> StructuralIndependenceRequirement {
        self.structural_independence
    }

    /// Returns the configured consecutive saturation-round requirement.
    #[must_use]
    pub const fn saturation_rounds(&self) -> SaturationRoundCount {
        self.saturation_rounds
    }

    /// Returns every strength this policy is allowed to admit.
    #[must_use]
    pub fn accepted_strengths(&self) -> &[OracleStrength] {
        &self.accepted_strengths
    }

    /// Returns the execution scopes this policy requires.
    #[must_use]
    pub fn required_execution_scopes(&self) -> &[AdmissionExecutionScope] {
        &self.required_execution_scopes
    }

    /// Returns the configured incomplete-budget outcome.
    #[must_use]
    pub const fn budget_exhaustion_outcome(&self) -> BudgetExhaustionOutcome {
        self.budget_exhaustion_outcome
    }
}

impl TryFrom<AdmissionPolicyWire> for AdmissionPolicyV1 {
    type Error = VerificationContractError;

    fn try_from(wire: AdmissionPolicyWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(AdmissionPolicyInput {
            mutant_set: wire.mutant_set,
            minimum_correct_variants: wire.minimum_correct_variants,
            minimum_incorrect_variants: wire.minimum_incorrect_variants,
            required_construction_classes: wire.required_construction_classes,
            required_fault_classes: wire.required_fault_classes,
            structural_independence: wire.structural_independence,
            saturation_rounds: wire.saturation_rounds,
            accepted_strengths: wire.accepted_strengths,
            required_execution_scopes: wire.required_execution_scopes,
            budget_exhaustion_outcome: wire.budget_exhaustion_outcome,
        })
    }
}

fn validate_nonempty_ordered<T: Ord>(
    values: &[T],
    field: &'static str,
) -> Result<(), VerificationContractError> {
    if values.is_empty() {
        return Err(VerificationContractError::EmptySet { field });
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(VerificationContractError::NonCanonicalSet { field });
    }
    Ok(())
}

/// Content domain for canonical admission-policy bytes.
pub enum AdmissionPolicyArtifact {}

impl ContentType for AdmissionPolicyArtifact {
    const DOMAIN: &'static str = "verification.admission-policy.v1";
}

/// Content domain for a frozen admission corpus manifest.
pub enum AdmissionCorpusArtifact {}

impl ContentType for AdmissionCorpusArtifact {
    const DOMAIN: &'static str = "verification.admission-corpus.v1";
}

/// Exact canonical non-negative decimal used at a numerical boundary.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct AllowanceMagnitude(String);

impl AllowanceMagnitude {
    /// Parses a canonical decimal without binary floating-point ambiguity.
    ///
    /// Accepted values use ordinary decimal notation, no sign or exponent, no leading integer
    /// zero, and no trailing fractional zero. `0` is the single zero representation.
    ///
    /// # Errors
    ///
    /// Rejects a non-canonical, negative, exponent, non-numeric, or oversized value.
    pub fn new(value: impl Into<String>) -> Result<Self, VerificationContractError> {
        let value = value.into();
        if !is_canonical_decimal(&value) {
            return Err(VerificationContractError::InvalidMagnitude);
        }
        Ok(Self(value))
    }

    /// Returns the exact decimal wire value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AllowanceMagnitude {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for AllowanceMagnitude {
    type Err = VerificationContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for AllowanceMagnitude {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

fn is_canonical_decimal(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_DECIMAL_LEN {
        return false;
    }
    let mut parts = value.split('.');
    let Some(integer) = parts.next() else {
        return false;
    };
    let fraction = parts.next();
    if parts.next().is_some()
        || integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || (integer.len() > 1 && integer.starts_with('0'))
    {
        return false;
    }
    match fraction {
        None => true,
        Some(fraction) => {
            !fraction.is_empty()
                && fraction.bytes().all(|byte| byte.is_ascii_digit())
                && !fraction.ends_with('0')
        }
    }
}

/// Source from which a numerical allowance value was obtained.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AllowanceProvenance {
    /// Executed correct-by-construction implementation family.
    MeasuredFamily,
    /// Executed adversarial search over valid inputs.
    MeasuredAdversarial,
    /// Imported convention not independently sufficient for strong admission.
    ExternalPrior,
    /// Proposed number without measurement.
    Asserted,
    /// Exact arithmetic or justified allowed-result-set membership.
    ExactOrSet,
}

/// What the recorded evidence justifies about an allowance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AllowanceAssurance {
    /// Mathematical or structural argument justifies the bound.
    ProvenBound,
    /// Every member of a declared finite domain was exercised.
    ExhaustiveFinite,
    /// Identity-disjoint derivation and validation corpora support an empirical bound.
    HeldOutValidated,
    /// Executed exploration has no independent generalization control.
    ExploratoryMeasured,
    /// Only an imported prior supports the value.
    PriorOnly,
    /// No admissible support exists.
    Unsupported,
}

/// Strongest claim class this allowance can support before other admission obligations are checked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AllowanceClaimClass {
    /// May support an unqualified domain-wide numerical claim.
    UnqualifiedDomainWide,
    /// May support only an explicitly empirical claim.
    Empirical,
    /// Cannot support a numerical pass.
    InsufficientEvidence,
}

/// Immutable numerical allowance with independent provenance and assurance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "NumericalAllowanceWire")]
pub struct NumericalAllowanceV1 {
    schema_version: VerificationSchemaV1,
    absolute: Option<AllowanceMagnitude>,
    relative: Option<AllowanceMagnitude>,
    provenance: AllowanceProvenance,
    assurance: AllowanceAssurance,
    derivation_corpora: Vec<ContentId<AdmissionCorpusArtifact>>,
    validation_corpora: Vec<ContentId<AdmissionCorpusArtifact>>,
    domain_regions: Vec<DomainRegionName>,
}

/// Constructor input for a numerical allowance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NumericalAllowanceInput {
    /// Optional exact absolute bound.
    pub absolute: Option<AllowanceMagnitude>,
    /// Optional exact relative bound.
    pub relative: Option<AllowanceMagnitude>,
    /// Independent origin classification.
    pub provenance: AllowanceProvenance,
    /// Independent assurance classification.
    pub assurance: AllowanceAssurance,
    /// Corpora used to derive the value, in strict identity order.
    pub derivation_corpora: Vec<ContentId<AdmissionCorpusArtifact>>,
    /// Identity-disjoint held-out corpora, in strict identity order.
    pub validation_corpora: Vec<ContentId<AdmissionCorpusArtifact>>,
    /// Domain regions covered by this allowance, in strict canonical order.
    pub domain_regions: Vec<DomainRegionName>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NumericalAllowanceWire {
    schema_version: VerificationSchemaV1,
    absolute: Option<AllowanceMagnitude>,
    relative: Option<AllowanceMagnitude>,
    provenance: AllowanceProvenance,
    assurance: AllowanceAssurance,
    derivation_corpora: Vec<ContentId<AdmissionCorpusArtifact>>,
    validation_corpora: Vec<ContentId<AdmissionCorpusArtifact>>,
    domain_regions: Vec<DomainRegionName>,
}

impl NumericalAllowanceV1 {
    /// Validates a complete numerical allowance.
    ///
    /// # Errors
    ///
    /// Rejects missing magnitudes, non-canonical identity/region sets, missing measured evidence,
    /// and any derivation/held-out corpus overlap.
    pub fn new(input: NumericalAllowanceInput) -> Result<Self, VerificationContractError> {
        if input.absolute.is_none() && input.relative.is_none() {
            return Err(VerificationContractError::MissingMagnitude);
        }
        validate_content_id_order(&input.derivation_corpora, "derivation corpora")?;
        validate_content_id_order(&input.validation_corpora, "validation corpora")?;
        validate_nonempty_ordered(&input.domain_regions, "domain regions")?;
        if matches!(
            input.provenance,
            AllowanceProvenance::MeasuredFamily | AllowanceProvenance::MeasuredAdversarial
        ) && input.derivation_corpora.is_empty()
        {
            return Err(VerificationContractError::MissingDerivationCorpus {
                evidence: "measured",
            });
        }
        if input.assurance == AllowanceAssurance::ExhaustiveFinite
            && input.derivation_corpora.is_empty()
        {
            return Err(VerificationContractError::MissingDerivationCorpus {
                evidence: "exhaustive finite",
            });
        }
        if input.assurance == AllowanceAssurance::HeldOutValidated {
            if input.derivation_corpora.is_empty() {
                return Err(VerificationContractError::MissingDerivationCorpus {
                    evidence: "held-out",
                });
            }
            if input.validation_corpora.is_empty() {
                return Err(VerificationContractError::MissingValidationCorpus);
            }
            if input.derivation_corpora.iter().any(|derivation| {
                input
                    .validation_corpora
                    .iter()
                    .any(|validation| validation == derivation)
            }) {
                return Err(VerificationContractError::CorpusOverlap);
            }
        }
        Ok(Self {
            schema_version: VerificationSchemaV1,
            absolute: input.absolute,
            relative: input.relative,
            provenance: input.provenance,
            assurance: input.assurance,
            derivation_corpora: input.derivation_corpora,
            validation_corpora: input.validation_corpora,
            domain_regions: input.domain_regions,
        })
    }

    /// Returns the exact optional absolute bound.
    #[must_use]
    pub const fn absolute(&self) -> Option<&AllowanceMagnitude> {
        self.absolute.as_ref()
    }

    /// Returns the exact optional relative bound.
    #[must_use]
    pub const fn relative(&self) -> Option<&AllowanceMagnitude> {
        self.relative.as_ref()
    }

    /// Returns how the value was obtained.
    #[must_use]
    pub const fn provenance(&self) -> AllowanceProvenance {
        self.provenance
    }

    /// Returns what the evidence can justify.
    #[must_use]
    pub const fn assurance(&self) -> AllowanceAssurance {
        self.assurance
    }

    /// Returns derivation corpus identities.
    #[must_use]
    pub fn derivation_corpora(&self) -> &[ContentId<AdmissionCorpusArtifact>] {
        &self.derivation_corpora
    }

    /// Returns independent validation corpus identities.
    #[must_use]
    pub fn validation_corpora(&self) -> &[ContentId<AdmissionCorpusArtifact>] {
        &self.validation_corpora
    }

    /// Returns the exact regions to which the allowance applies.
    #[must_use]
    pub fn domain_regions(&self) -> &[DomainRegionName] {
        &self.domain_regions
    }

    /// Computes the strongest possible numerical claim from provenance and assurance.
    ///
    /// Other admission obligations can only weaken this result. In particular, asserted and
    /// external-prior-only values cannot be upgraded by forged assurance metadata.
    #[must_use]
    pub const fn maximum_claim_class(&self) -> AllowanceClaimClass {
        if matches!(
            self.provenance,
            AllowanceProvenance::Asserted | AllowanceProvenance::ExternalPrior
        ) {
            return AllowanceClaimClass::InsufficientEvidence;
        }
        match self.assurance {
            AllowanceAssurance::ProvenBound | AllowanceAssurance::ExhaustiveFinite => {
                AllowanceClaimClass::UnqualifiedDomainWide
            }
            AllowanceAssurance::HeldOutValidated => AllowanceClaimClass::Empirical,
            AllowanceAssurance::ExploratoryMeasured
            | AllowanceAssurance::PriorOnly
            | AllowanceAssurance::Unsupported => AllowanceClaimClass::InsufficientEvidence,
        }
    }
}

impl TryFrom<NumericalAllowanceWire> for NumericalAllowanceV1 {
    type Error = VerificationContractError;

    fn try_from(wire: NumericalAllowanceWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(NumericalAllowanceInput {
            absolute: wire.absolute,
            relative: wire.relative,
            provenance: wire.provenance,
            assurance: wire.assurance,
            derivation_corpora: wire.derivation_corpora,
            validation_corpora: wire.validation_corpora,
            domain_regions: wire.domain_regions,
        })
    }
}

fn validate_content_id_order<T: ContentType>(
    values: &[ContentId<T>],
    field: &'static str,
) -> Result<(), VerificationContractError> {
    if values
        .windows(2)
        .any(|pair| pair[0].to_wire() >= pair[1].to_wire())
    {
        return Err(VerificationContractError::NonCanonicalSet { field });
    }
    Ok(())
}

/// Content domain for canonical numerical-allowance bytes.
pub enum NumericalAllowanceArtifact {}

impl ContentType for NumericalAllowanceArtifact {
    const DOMAIN: &'static str = "verification.numerical-allowance.v1";
}

mod admission;
mod candidate;
mod mutation;
mod proposal;

pub use admission::{
    AdmissionAdjudicationArtifact, AdmissionAssumptionV1, AdmissionControlArtifact,
    AdmissionCoverageArtifact, AdmissionDecisionV1, AdmissionDisagreementArtifact,
    AdmissionDisagreementDispositionV1, AdmissionEnvironmentArtifact,
    AdmissionProofFailureArtifact, AdmissionReceiptArtifact, AdmissionReceiptV1,
    AdmissionRevalidationPolicyV1, AdmissionRevalidationTriggerV1,
    AdmissionSaturationEvidenceArtifact, AdmissionSaturationRoundV1, AdmissionUnverifiedClaimV1,
    AdmissionVariantTrialArtifact, AdmittedDomainArtifact, AdmittedDomainExclusionArtifact,
    AdmittedDomainV1, AdmittedOracleArtifact, AdmittedOracleV1, AdmittedReceiptInput,
    PreparedAdmissionReceipt, PreparedAdmittedDomain, PreparedAdmittedOracle,
    SourceAdmissionObservationArtifact, prepare_admission_receipt, prepare_admitted_domain,
    prepare_admitted_oracle,
};
pub use candidate::{
    CandidateArtifact, CandidateBuildArtifact, CandidateComparisonArtifact,
    CandidateFailedCaseArtifact, CandidateRunArtifact, CandidateSourceArtifact,
    CandidateVerdictArtifact, CandidateVerdictInput, CandidateVerdictOutcomeV1, CandidateVerdictV1,
    PreparedCandidateVerdict, prepare_candidate_verdict,
};

pub use mutation::{
    GenericMutantSetArtifact, GenericMutantSetV1, MutationCaseArtifact, MutationComparisonArtifact,
    MutationDetection, MutationExecutionArtifact, MutationGridArtifact, MutationGridCellV1,
    MutationGridError, MutationGridProofArtifact, MutationGridProofFailureV1, MutationGridProofV1,
    MutationGridV1, MutationInjectionArtifact, MutationSizing, MutationTrialResultV1,
    MutationTrialV1, NonInjectableReasonArtifact, PreparedGenericMutantSet, PreparedMutationGrid,
    PreparedMutationGridProof, TrustedMutantDefinitionArtifact, TrustedMutantV1,
    prepare_generic_mutant_set, prepare_mutation_grid, recompute_mutation_grid_proof,
};

pub use proposal::{
    ArtifactAuthorId, ArtifactAuthorshipV1, AuthorshipOrigin, CallerDomainBodyArtifact,
    CallerDomainEvidenceArtifact, ConstructionClaimArtifact, ConstructionClaimInput,
    ConstructionClaimV1, ConstructionEvidenceArtifact, ConstructionJustification,
    ConstructionPrerequisiteArtifact, CorpusCaseArtifact, CorpusCaseEntryV1,
    CorpusCaseProvenanceArtifact, CorpusCaseSource, CorpusProposalArtifact, CorpusProposalInput,
    CorpusProposalV1, CoverageObligationArtifact, DeclaredDomainArtifact, DeclaredDomainV1,
    DomainDifferenceArtifact, DomainRefinementArtifact, DomainRefinementEvidenceArtifact,
    DomainRefinementV1, DomainUnknownArtifact, FaultInjectionEvidenceArtifact,
    ImplementationBundleArtifact, ImplementationVariantArtifact, ImplementationVariantV1,
    LicenseProvenanceArtifact, ModelConfigurationArtifact, ObservationPlanArtifact,
    OracleProposalArtifact, OracleProposalInput, OracleProposalV1, OracleTaskInputArtifact,
    PropertyRelationArtifact, ReferenceArtifact, SourceAdmissionPlanArtifact,
    TransformationKindName, ValidFamilyPlanArtifact, VariantExpectation,
};

#[cfg(test)]
mod tests {
    use cairn_protocol::{ContentId, ContentType};

    use super::{
        AdmissionCorpusArtifact, AdmissionExecutionScope, AdmissionPolicyArtifact,
        AdmissionPolicyInput, AdmissionPolicyV1, AllowanceAssurance, AllowanceClaimClass,
        AllowanceMagnitude, AllowanceProvenance, BudgetExhaustionOutcome, ConstructionClassName,
        CorrectVariantMinimum, DomainRegionName, FaultClassName, GenericMutantSetArtifact,
        IncorrectVariantMinimum, NumericalAllowanceArtifact, NumericalAllowanceInput,
        NumericalAllowanceV1, OracleStrength, SaturationRoundCount,
        StructuralIndependenceRequirement, VerificationContractError,
    };

    fn policy(minimum_correct: u32) -> AdmissionPolicyV1 {
        AdmissionPolicyV1::new(AdmissionPolicyInput {
            mutant_set: ContentId::<GenericMutantSetArtifact>::derive(b"policy-mutants")
                .expect("mutant set"),
            minimum_correct_variants: CorrectVariantMinimum::new(minimum_correct)
                .expect("correct minimum"),
            minimum_incorrect_variants: IncorrectVariantMinimum::new(3).expect("incorrect minimum"),
            required_construction_classes: vec![
                ConstructionClassName::new("linear-order").expect("class"),
                ConstructionClassName::new("tree-order").expect("class"),
            ],
            required_fault_classes: vec![
                FaultClassName::new("offset").expect("class"),
                FaultClassName::new("zero-output").expect("class"),
            ],
            structural_independence: StructuralIndependenceRequirement::DistinctConstructionClaims,
            saturation_rounds: SaturationRoundCount::new(2).expect("rounds"),
            accepted_strengths: vec![OracleStrength::Reference],
            required_execution_scopes: vec![
                AdmissionExecutionScope::ObservationPipeline,
                AdmissionExecutionScope::Implementation,
            ],
            budget_exhaustion_outcome: BudgetExhaustionOutcome::Unverifiable,
        })
        .expect("policy")
    }

    fn corpus(seed: &[u8]) -> ContentId<AdmissionCorpusArtifact> {
        ContentId::derive(seed).expect("corpus identity")
    }

    fn ordered_corpora(
        first: ContentId<AdmissionCorpusArtifact>,
        second: ContentId<AdmissionCorpusArtifact>,
    ) -> Vec<ContentId<AdmissionCorpusArtifact>> {
        let mut values = vec![first, second];
        values.sort_by_key(ContentId::to_wire);
        values
    }

    fn allowance(
        provenance: AllowanceProvenance,
        assurance: AllowanceAssurance,
        derivation_corpora: Vec<ContentId<AdmissionCorpusArtifact>>,
        validation_corpora: Vec<ContentId<AdmissionCorpusArtifact>>,
    ) -> Result<NumericalAllowanceV1, VerificationContractError> {
        NumericalAllowanceV1::new(NumericalAllowanceInput {
            absolute: Some(AllowanceMagnitude::new("0.001").expect("magnitude")),
            relative: None,
            provenance,
            assurance,
            derivation_corpora,
            validation_corpora,
            domain_regions: vec![DomainRegionName::new("all-declared-inputs").expect("region")],
        })
    }

    #[test]
    fn admission_policy_is_strict_v1_configuration_and_every_policy_change_has_a_new_identity() {
        let original_policy = policy(2);
        let bytes = cairn_codec::to_vec(&original_policy).expect("canonical policy");
        let decoded: AdmissionPolicyV1 =
            cairn_codec::from_slice(&bytes).expect("strict round trip");
        assert_eq!(decoded, original_policy);

        let changed = policy(4);
        let changed_bytes = cairn_codec::to_vec(&changed).expect("changed policy");
        assert_ne!(
            ContentId::<AdmissionPolicyArtifact>::derive(&bytes).expect("policy identity"),
            ContentId::<AdmissionPolicyArtifact>::derive(&changed_bytes)
                .expect("changed policy identity")
        );

        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        value["schema_version"] = serde_json::json!(2);
        assert!(serde_json::from_value::<AdmissionPolicyV1>(value.clone()).is_err());
        value["schema_version"] = serde_json::json!(1);
        value["unknown"] = serde_json::json!(true);
        assert!(serde_json::from_value::<AdmissionPolicyV1>(value).is_err());
    }

    #[test]
    fn policy_sets_and_counts_fail_closed_instead_of_becoming_verifier_defaults() {
        assert!(CorrectVariantMinimum::new(0).is_err());
        let mut input = AdmissionPolicyInput {
            mutant_set: ContentId::<GenericMutantSetArtifact>::derive(b"policy-mutants")
                .expect("mutant set"),
            minimum_correct_variants: CorrectVariantMinimum::new(2).expect("minimum"),
            minimum_incorrect_variants: IncorrectVariantMinimum::new(3).expect("minimum"),
            required_construction_classes: vec![
                ConstructionClassName::new("tree-order").expect("class"),
                ConstructionClassName::new("linear-order").expect("class"),
            ],
            required_fault_classes: vec![FaultClassName::new("offset").expect("class")],
            structural_independence: StructuralIndependenceRequirement::DistinctConstructionClaims,
            saturation_rounds: SaturationRoundCount::new(2).expect("rounds"),
            accepted_strengths: vec![OracleStrength::Reference],
            required_execution_scopes: vec![AdmissionExecutionScope::Implementation],
            budget_exhaustion_outcome: BudgetExhaustionOutcome::Reject,
        };
        assert!(matches!(
            AdmissionPolicyV1::new(input.clone()),
            Err(VerificationContractError::NonCanonicalSet { .. })
        ));
        input.required_construction_classes.clear();
        assert!(matches!(
            AdmissionPolicyV1::new(input),
            Err(VerificationContractError::EmptySet { .. })
        ));
    }

    #[test]
    fn held_out_validation_requires_identity_disjoint_corpora_and_is_empirical_only() {
        let derivation = corpus(b"derivation");
        assert_eq!(
            allowance(
                AllowanceProvenance::MeasuredFamily,
                AllowanceAssurance::HeldOutValidated,
                vec![derivation],
                vec![derivation],
            ),
            Err(VerificationContractError::CorpusOverlap)
        );

        let validation = corpus(b"validation");
        let allowance = allowance(
            AllowanceProvenance::MeasuredFamily,
            AllowanceAssurance::HeldOutValidated,
            vec![derivation],
            vec![validation],
        )
        .expect("held-out allowance");
        assert_eq!(
            allowance.maximum_claim_class(),
            AllowanceClaimClass::Empirical
        );
        let bytes = cairn_codec::to_vec(&allowance).expect("allowance bytes");
        assert_eq!(
            cairn_codec::from_slice::<NumericalAllowanceV1>(&bytes).expect("allowance round trip"),
            allowance
        );
    }

    #[test]
    fn asserted_or_prior_values_cannot_be_upgraded_by_assurance_metadata() {
        for provenance in [
            AllowanceProvenance::Asserted,
            AllowanceProvenance::ExternalPrior,
        ] {
            let allowance = allowance(
                provenance,
                AllowanceAssurance::ProvenBound,
                Vec::new(),
                Vec::new(),
            )
            .expect("record the independent classifications");
            assert_eq!(
                allowance.maximum_claim_class(),
                AllowanceClaimClass::InsufficientEvidence
            );
        }
    }

    #[test]
    fn only_proven_or_exhaustive_evidence_can_support_an_unqualified_claim() {
        let exhaustive = allowance(
            AllowanceProvenance::MeasuredAdversarial,
            AllowanceAssurance::ExhaustiveFinite,
            vec![corpus(b"finite-domain")],
            Vec::new(),
        )
        .expect("exhaustive allowance");
        assert_eq!(
            exhaustive.maximum_claim_class(),
            AllowanceClaimClass::UnqualifiedDomainWide
        );

        let exploratory = allowance(
            AllowanceProvenance::MeasuredFamily,
            AllowanceAssurance::ExploratoryMeasured,
            vec![corpus(b"exploration")],
            Vec::new(),
        )
        .expect("exploratory allowance");
        assert_eq!(
            exploratory.maximum_claim_class(),
            AllowanceClaimClass::InsufficientEvidence
        );
    }

    #[test]
    fn exact_decimal_and_typed_content_boundaries_are_canonical() {
        for invalid in ["", "00", "01", ".1", "1.", "1.0", "+1", "-1", "1e-3"] {
            assert!(
                AllowanceMagnitude::new(invalid).is_err(),
                "accepted {invalid}"
            );
        }
        for valid in ["0", "1", "1.25", "0.001"] {
            assert_eq!(
                AllowanceMagnitude::new(valid)
                    .expect("valid magnitude")
                    .as_str(),
                valid
            );
        }

        assert_ne!(
            AdmissionPolicyArtifact::DOMAIN,
            NumericalAllowanceArtifact::DOMAIN
        );
        let first = corpus(b"first");
        let second = corpus(b"second");
        let ordered = ordered_corpora(first, second);
        let mut reversed = ordered.clone();
        reversed.reverse();
        assert!(
            allowance(
                AllowanceProvenance::MeasuredFamily,
                AllowanceAssurance::ExploratoryMeasured,
                reversed,
                Vec::new(),
            )
            .is_err()
        );
    }
}
