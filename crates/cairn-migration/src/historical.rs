//! Immutable historical-failure records and mandatory regression obligations.

use std::{fmt, str::FromStr};

use cairn_protocol::{ContentId, ContentType};
use cairn_verification::{CallerDomainBodyArtifact, LicenseProvenanceArtifact};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

use crate::domain::StatusCode;

const MAX_HISTORICAL_LABEL_LEN: usize = 128;

/// Failure to construct or validate historical regression evidence.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum HistoricalFailureContractError {
    /// Only the current pre-release V1 schema is accepted.
    #[error("historical failure schema version must be 1")]
    UnsupportedSchemaVersion,
    /// A semantic label was empty, oversized, or non-canonical.
    #[error("{kind} is not a valid historical-failure label")]
    InvalidLabel {
        /// Label kind that failed.
        kind: &'static str,
    },
    /// A required canonical set was empty.
    #[error("{field} must contain at least one value")]
    EmptySet {
        /// Set that failed.
        field: &'static str,
    },
    /// A canonical set was duplicated or out of order.
    #[error("{field} must be in strict canonical order without duplicates")]
    NonCanonicalSet {
        /// Set that failed.
        field: &'static str,
    },
    /// Oracle and target failure scopes were combined with the wrong execution stage.
    #[error("historical failure scope is incompatible with its observed stage")]
    ScopeStageMismatch,
    /// The required detector does not exercise the stage at which the failure was observed.
    #[error("historical detection requirement is incompatible with the recorded stage")]
    DetectionStageMismatch,
    /// An obligation was checked against different record bytes.
    #[error("historical obligation record identity does not match the supplied record")]
    RecordIdentityMismatch,
    /// Copied obligation metadata disagrees with the cited record.
    #[error("historical obligation metadata does not match the supplied record")]
    RecordMetadataMismatch,
    /// A coverage set mixed obligations from another domain family.
    #[error("historical coverage obligation belongs to a different domain family")]
    DomainFamilyMismatch,
    /// Canonical encoding or identity derivation failed.
    #[error("historical failure codec error: {message}")]
    Codec {
        /// Adapter-neutral diagnostic.
        message: String,
    },
}

fn validate_label(value: &str, kind: &'static str) -> Result<(), HistoricalFailureContractError> {
    if value.is_empty()
        || value.len() > MAX_HISTORICAL_LABEL_LEN
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'.' | b'_' | b'/')
        })
    {
        return Err(HistoricalFailureContractError::InvalidLabel { kind });
    }
    Ok(())
}

macro_rules! historical_label {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a validated semantic label.
            ///
            /// # Errors
            ///
            /// Rejects empty, oversized, or non-canonical labels.
            pub fn new(value: impl Into<String>) -> Result<Self, HistoricalFailureContractError> {
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
            type Err = HistoricalFailureContractError;

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

historical_label!(
    /// Stable semantic class of one historical failure.
    HistoricalFailureClassName,
    "historical failure class"
);
historical_label!(
    /// Product-domain family to which a historical regression applies.
    MigrationDomainFamilyName,
    "migration domain family"
);
historical_label!(
    /// Target mechanism implicated by an observed target failure.
    TargetMechanismName,
    "target mechanism"
);
historical_label!(
    /// Oracle mechanism implicated by an observed adjudication failure.
    OracleFailureMechanismName,
    "oracle failure mechanism"
);
historical_label!(
    /// Stable compiler or linker diagnostic category required for detection.
    HistoricalDiagnosticClassName,
    "historical diagnostic class"
);
historical_label!(
    /// Stable behavioral observation category required for detection.
    HistoricalObservationClassName,
    "historical observation class"
);

/// Exact scope in which a historical failure was observed.
///
/// ```compile_fail
/// use cairn_migration::{HistoricalFailureClassName, TargetMechanismName};
///
/// fn require_failure_class(_: HistoricalFailureClassName) {}
/// let mechanism = TargetMechanismName::new("gm-addr").unwrap();
/// require_failure_class(mechanism);
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum HistoricalFailureScope {
    /// Failure in target build or execution machinery.
    Target {
        /// Implicated mechanisms in strict canonical order.
        mechanisms: Vec<TargetMechanismName>,
    },
    /// Failure in oracle comparison or adjudication logic.
    Oracle {
        /// Exact oracle mechanism.
        mechanism: OracleFailureMechanismName,
    },
}

impl HistoricalFailureScope {
    fn validate(&self) -> Result<(), HistoricalFailureContractError> {
        match self {
            Self::Target { mechanisms } => {
                validate_non_empty_canonical(mechanisms, "target mechanisms")
            }
            Self::Oracle { .. } => Ok(()),
        }
    }

    fn is_target(&self) -> bool {
        matches!(self, Self::Target { .. })
    }
}

/// Stage at which a historical failure was actually observed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HistoricalValidationStage {
    /// Trusted oracle comparison or adjudication.
    OracleComparison,
    /// Target-source compilation.
    TargetCompilation,
    /// Target linkage or binary assembly.
    TargetLink,
    /// Target invocation/status boundary.
    TargetInvocation,
    /// Target output or device-observation boundary.
    TargetObservation,
}

impl HistoricalValidationStage {
    const fn is_target(self) -> bool {
        !matches!(self, Self::OracleComparison)
    }
}

/// Required detection that turns one historical record into a regression obligation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum HistoricalDetectionRequirement {
    /// The trusted oracle must reproduce and then prevent a known verdict divergence.
    OracleVerdictDivergence,
    /// Target compilation must emit this diagnostic category.
    CompileDiagnostic {
        /// Required diagnostic class.
        diagnostic_class: HistoricalDiagnosticClassName,
    },
    /// Target linkage must emit this diagnostic category.
    LinkDiagnostic {
        /// Required diagnostic class.
        diagnostic_class: HistoricalDiagnosticClassName,
    },
    /// Target invocation must fail, without claiming a particular status.
    AnyInvocationFailure,
    /// Target invocation must produce an exact typed status code.
    InvocationStatus {
        /// Required status code.
        status: StatusCode,
    },
    /// Target output observation must detect this behavioral class.
    OutputObservation {
        /// Required observation class.
        observation_class: HistoricalObservationClassName,
    },
}

impl HistoricalDetectionRequirement {
    const fn stage(&self) -> HistoricalValidationStage {
        match self {
            Self::OracleVerdictDivergence => HistoricalValidationStage::OracleComparison,
            Self::CompileDiagnostic { .. } => HistoricalValidationStage::TargetCompilation,
            Self::LinkDiagnostic { .. } => HistoricalValidationStage::TargetLink,
            Self::AnyInvocationFailure | Self::InvocationStatus { .. } => {
                HistoricalValidationStage::TargetInvocation
            }
            Self::OutputObservation { .. } => HistoricalValidationStage::TargetObservation,
        }
    }
}

/// Content domain for immutable evidence from which a historical record was reconstructed.
pub enum HistoricalFailureEvidenceArtifact {}

impl ContentType for HistoricalFailureEvidenceArtifact {
    const DOMAIN: &'static str = "migration.historical-failure-evidence.v1";
}

/// Content domain for an independently runnable historical reproduction fixture.
pub enum HistoricalReproductionArtifact {}

impl ContentType for HistoricalReproductionArtifact {
    const DOMAIN: &'static str = "migration.historical-reproduction.v1";
}

/// Content domain for the original observed failure payload or receipt.
pub enum HistoricalObservedFailureArtifact {}

impl ContentType for HistoricalObservedFailureArtifact {
    const DOMAIN: &'static str = "migration.historical-observed-failure.v1";
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HistoricalSchemaV1;

impl Serialize for HistoricalSchemaV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(1)
    }
}

impl<'de> Deserialize<'de> for HistoricalSchemaV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match u32::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(de::Error::custom(
                HistoricalFailureContractError::UnsupportedSchemaVersion,
            )),
        }
    }
}

/// Constructor input for one immutable historical failure record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalFailureRecordInput {
    /// Stable failure class.
    pub failure_class: HistoricalFailureClassName,
    /// Product-domain family to which the record applies.
    pub domain_family: MigrationDomainFamilyName,
    /// Target or oracle mechanism scope.
    pub scope: HistoricalFailureScope,
    /// Stage at which the failure was observed.
    pub observed_stage: HistoricalValidationStage,
    /// Original evidence identities in strict canonical order.
    pub source_evidence: Vec<ContentId<HistoricalFailureEvidenceArtifact>>,
    /// Exact original failure observation.
    pub observed_failure: ContentId<HistoricalObservedFailureArtifact>,
    /// Independently runnable reproduction fixture.
    pub reproduction_fixture: ContentId<HistoricalReproductionArtifact>,
    /// License/source provenance for imported material.
    pub license_provenance: ContentId<LicenseProvenanceArtifact>,
}

/// Immutable provenance-bearing historical failure record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "HistoricalFailureRecordWire")]
pub struct HistoricalFailureRecordV1 {
    schema_version: HistoricalSchemaV1,
    failure_class: HistoricalFailureClassName,
    domain_family: MigrationDomainFamilyName,
    scope: HistoricalFailureScope,
    observed_stage: HistoricalValidationStage,
    source_evidence: Vec<ContentId<HistoricalFailureEvidenceArtifact>>,
    observed_failure: ContentId<HistoricalObservedFailureArtifact>,
    reproduction_fixture: ContentId<HistoricalReproductionArtifact>,
    license_provenance: ContentId<LicenseProvenanceArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalFailureRecordWire {
    schema_version: HistoricalSchemaV1,
    failure_class: HistoricalFailureClassName,
    domain_family: MigrationDomainFamilyName,
    scope: HistoricalFailureScope,
    observed_stage: HistoricalValidationStage,
    source_evidence: Vec<ContentId<HistoricalFailureEvidenceArtifact>>,
    observed_failure: ContentId<HistoricalObservedFailureArtifact>,
    reproduction_fixture: ContentId<HistoricalReproductionArtifact>,
    license_provenance: ContentId<LicenseProvenanceArtifact>,
}

impl HistoricalFailureRecordV1 {
    /// Validates and constructs one historical record.
    ///
    /// # Errors
    ///
    /// Rejects missing/non-canonical evidence and oracle/target stage-scope disagreement.
    pub fn new(
        input: HistoricalFailureRecordInput,
    ) -> Result<Self, HistoricalFailureContractError> {
        input.scope.validate()?;
        if input.scope.is_target() != input.observed_stage.is_target() {
            return Err(HistoricalFailureContractError::ScopeStageMismatch);
        }
        validate_non_empty_content_ids(&input.source_evidence, "historical source evidence")?;
        Ok(Self {
            schema_version: HistoricalSchemaV1,
            failure_class: input.failure_class,
            domain_family: input.domain_family,
            scope: input.scope,
            observed_stage: input.observed_stage,
            source_evidence: input.source_evidence,
            observed_failure: input.observed_failure,
            reproduction_fixture: input.reproduction_fixture,
            license_provenance: input.license_provenance,
        })
    }

    /// Returns the stable failure class.
    #[must_use]
    pub const fn failure_class(&self) -> &HistoricalFailureClassName {
        &self.failure_class
    }

    /// Returns the applicable migration-domain family.
    #[must_use]
    pub const fn domain_family(&self) -> &MigrationDomainFamilyName {
        &self.domain_family
    }

    /// Returns the observed validation stage.
    #[must_use]
    pub const fn observed_stage(&self) -> HistoricalValidationStage {
        self.observed_stage
    }

    /// Returns whether the failure came from target machinery or oracle logic.
    #[must_use]
    pub const fn scope(&self) -> &HistoricalFailureScope {
        &self.scope
    }

    /// Returns the original evidence identities in canonical order.
    #[must_use]
    pub fn source_evidence(&self) -> &[ContentId<HistoricalFailureEvidenceArtifact>] {
        &self.source_evidence
    }

    /// Returns the exact original observed failure.
    #[must_use]
    pub const fn observed_failure(&self) -> ContentId<HistoricalObservedFailureArtifact> {
        self.observed_failure
    }

    /// Returns the exact reproduction fixture.
    #[must_use]
    pub const fn reproduction_fixture(&self) -> ContentId<HistoricalReproductionArtifact> {
        self.reproduction_fixture
    }

    /// Returns the license/source provenance for imported material.
    #[must_use]
    pub const fn license_provenance(&self) -> ContentId<LicenseProvenanceArtifact> {
        self.license_provenance
    }
}

impl TryFrom<HistoricalFailureRecordWire> for HistoricalFailureRecordV1 {
    type Error = HistoricalFailureContractError;

    fn try_from(wire: HistoricalFailureRecordWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(HistoricalFailureRecordInput {
            failure_class: wire.failure_class,
            domain_family: wire.domain_family,
            scope: wire.scope,
            observed_stage: wire.observed_stage,
            source_evidence: wire.source_evidence,
            observed_failure: wire.observed_failure,
            reproduction_fixture: wire.reproduction_fixture,
            license_provenance: wire.license_provenance,
        })
    }
}

/// Content identity domain for historical failure records.
pub enum HistoricalFailureRecordArtifact {}

impl ContentType for HistoricalFailureRecordArtifact {
    const DOMAIN: &'static str = "migration.historical-failure-record.v1";
}

/// One mandatory regression obligation derived from an exact historical record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "HistoricalFailureObligationWire")]
pub struct HistoricalFailureObligationV1 {
    schema_version: HistoricalSchemaV1,
    record: ContentId<HistoricalFailureRecordArtifact>,
    failure_class: HistoricalFailureClassName,
    domain_family: MigrationDomainFamilyName,
    observed_stage: HistoricalValidationStage,
    required_detection: HistoricalDetectionRequirement,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalFailureObligationWire {
    schema_version: HistoricalSchemaV1,
    record: ContentId<HistoricalFailureRecordArtifact>,
    failure_class: HistoricalFailureClassName,
    domain_family: MigrationDomainFamilyName,
    observed_stage: HistoricalValidationStage,
    required_detection: HistoricalDetectionRequirement,
}

impl HistoricalFailureObligationV1 {
    /// Binds a required detector to the canonical identity and metadata of one record.
    ///
    /// # Errors
    ///
    /// Rejects a detector that operates at a different stage from the recorded failure or a
    /// canonical encoding failure.
    pub fn from_record(
        record: &HistoricalFailureRecordV1,
        required_detection: HistoricalDetectionRequirement,
    ) -> Result<Self, HistoricalFailureContractError> {
        if required_detection.stage() != record.observed_stage() {
            return Err(HistoricalFailureContractError::DetectionStageMismatch);
        }
        let bytes =
            cairn_codec::to_vec(record).map_err(|error| HistoricalFailureContractError::Codec {
                message: error.to_string(),
            })?;
        let record_id =
            ContentId::<HistoricalFailureRecordArtifact>::derive(&bytes).map_err(|error| {
                HistoricalFailureContractError::Codec {
                    message: error.to_string(),
                }
            })?;
        Ok(Self {
            schema_version: HistoricalSchemaV1,
            record: record_id,
            failure_class: record.failure_class().clone(),
            domain_family: record.domain_family().clone(),
            observed_stage: record.observed_stage(),
            required_detection,
        })
    }

    fn from_parts(
        record: ContentId<HistoricalFailureRecordArtifact>,
        failure_class: HistoricalFailureClassName,
        domain_family: MigrationDomainFamilyName,
        observed_stage: HistoricalValidationStage,
        required_detection: HistoricalDetectionRequirement,
    ) -> Result<Self, HistoricalFailureContractError> {
        if required_detection.stage() != observed_stage {
            return Err(HistoricalFailureContractError::DetectionStageMismatch);
        }
        Ok(Self {
            schema_version: HistoricalSchemaV1,
            record,
            failure_class,
            domain_family,
            observed_stage,
            required_detection,
        })
    }

    /// Recomputes the record identity and verifies all copied applicability metadata.
    ///
    /// # Errors
    ///
    /// Rejects different record bytes or conflicting class/family/stage metadata.
    pub fn validate_record(
        &self,
        record: &HistoricalFailureRecordV1,
    ) -> Result<(), HistoricalFailureContractError> {
        let bytes =
            cairn_codec::to_vec(record).map_err(|error| HistoricalFailureContractError::Codec {
                message: error.to_string(),
            })?;
        let record_id =
            ContentId::<HistoricalFailureRecordArtifact>::derive(&bytes).map_err(|error| {
                HistoricalFailureContractError::Codec {
                    message: error.to_string(),
                }
            })?;
        if record_id != self.record {
            return Err(HistoricalFailureContractError::RecordIdentityMismatch);
        }
        if record.failure_class() != &self.failure_class
            || record.domain_family() != &self.domain_family
            || record.observed_stage() != self.observed_stage
        {
            return Err(HistoricalFailureContractError::RecordMetadataMismatch);
        }
        Ok(())
    }

    /// Returns the cited record identity.
    #[must_use]
    pub const fn record(&self) -> ContentId<HistoricalFailureRecordArtifact> {
        self.record
    }

    /// Returns the stable failure class copied from the record.
    #[must_use]
    pub const fn failure_class(&self) -> &HistoricalFailureClassName {
        &self.failure_class
    }

    /// Returns the applicable domain family copied from the record.
    #[must_use]
    pub const fn domain_family(&self) -> &MigrationDomainFamilyName {
        &self.domain_family
    }

    /// Returns the required detector.
    #[must_use]
    pub const fn required_detection(&self) -> &HistoricalDetectionRequirement {
        &self.required_detection
    }

    /// Returns the execution/verification stage that must detect the regression.
    #[must_use]
    pub const fn required_stage(&self) -> HistoricalValidationStage {
        self.required_detection.stage()
    }
}

impl TryFrom<HistoricalFailureObligationWire> for HistoricalFailureObligationV1 {
    type Error = HistoricalFailureContractError;

    fn try_from(wire: HistoricalFailureObligationWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::from_parts(
            wire.record,
            wire.failure_class,
            wire.domain_family,
            wire.observed_stage,
            wire.required_detection,
        )
    }
}

/// Content identity domain for one historical regression obligation.
pub enum HistoricalFailureObligationArtifact {}

impl ContentType for HistoricalFailureObligationArtifact {
    const DOMAIN: &'static str = "migration.historical-failure-obligation.v1";
}

/// Canonical historical regression obligations attached to one exact caller domain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "HistoricalFailureCoverageWire")]
pub struct HistoricalFailureCoverageV1 {
    schema_version: HistoricalSchemaV1,
    domain: ContentId<CallerDomainBodyArtifact>,
    domain_family: MigrationDomainFamilyName,
    obligations: Vec<HistoricalFailureObligationV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalFailureCoverageWire {
    schema_version: HistoricalSchemaV1,
    domain: ContentId<CallerDomainBodyArtifact>,
    domain_family: MigrationDomainFamilyName,
    obligations: Vec<HistoricalFailureObligationV1>,
}

impl HistoricalFailureCoverageV1 {
    /// Creates a non-empty canonical obligation set for one exact domain and family.
    ///
    /// # Errors
    ///
    /// Rejects empty, duplicate/out-of-order, or cross-family obligations.
    pub fn new(
        domain: ContentId<CallerDomainBodyArtifact>,
        domain_family: MigrationDomainFamilyName,
        obligations: Vec<HistoricalFailureObligationV1>,
    ) -> Result<Self, HistoricalFailureContractError> {
        if obligations.is_empty() {
            return Err(HistoricalFailureContractError::EmptySet {
                field: "historical failure obligations",
            });
        }
        if obligations
            .iter()
            .any(|obligation| obligation.domain_family() != &domain_family)
        {
            return Err(HistoricalFailureContractError::DomainFamilyMismatch);
        }
        if obligations
            .windows(2)
            .any(|pair| obligation_order_key(&pair[0]) >= obligation_order_key(&pair[1]))
        {
            return Err(HistoricalFailureContractError::NonCanonicalSet {
                field: "historical failure obligations",
            });
        }
        Ok(Self {
            schema_version: HistoricalSchemaV1,
            domain,
            domain_family,
            obligations,
        })
    }

    /// Returns the exact caller domain receiving these obligations.
    #[must_use]
    pub const fn domain(&self) -> ContentId<CallerDomainBodyArtifact> {
        self.domain
    }

    /// Returns the domain-family classification.
    #[must_use]
    pub const fn domain_family(&self) -> &MigrationDomainFamilyName {
        &self.domain_family
    }

    /// Returns obligations in strict class/record order.
    #[must_use]
    pub fn obligations(&self) -> &[HistoricalFailureObligationV1] {
        &self.obligations
    }
}

impl TryFrom<HistoricalFailureCoverageWire> for HistoricalFailureCoverageV1 {
    type Error = HistoricalFailureContractError;

    fn try_from(wire: HistoricalFailureCoverageWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        Self::new(wire.domain, wire.domain_family, wire.obligations)
    }
}

/// Content identity domain for an exact-domain historical coverage set.
pub enum HistoricalFailureCoverageArtifact {}

impl ContentType for HistoricalFailureCoverageArtifact {
    const DOMAIN: &'static str = "migration.historical-failure-coverage.v1";
}

fn obligation_order_key(
    obligation: &HistoricalFailureObligationV1,
) -> (&HistoricalFailureClassName, String) {
    (obligation.failure_class(), obligation.record().to_wire())
}

fn validate_non_empty_canonical<T: Ord>(
    values: &[T],
    field: &'static str,
) -> Result<(), HistoricalFailureContractError> {
    if values.is_empty() {
        return Err(HistoricalFailureContractError::EmptySet { field });
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(HistoricalFailureContractError::NonCanonicalSet { field });
    }
    Ok(())
}

fn validate_non_empty_content_ids<T: ContentType>(
    values: &[ContentId<T>],
    field: &'static str,
) -> Result<(), HistoricalFailureContractError> {
    if values.is_empty() {
        return Err(HistoricalFailureContractError::EmptySet { field });
    }
    if values
        .windows(2)
        .any(|pair| pair[0].to_wire() >= pair[1].to_wire())
    {
        return Err(HistoricalFailureContractError::NonCanonicalSet { field });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use cairn_protocol::{ContentId, ContentType};
    use cairn_verification::{CallerDomainBodyArtifact, LicenseProvenanceArtifact};

    use super::{
        HistoricalDetectionRequirement, HistoricalDiagnosticClassName, HistoricalFailureClassName,
        HistoricalFailureContractError, HistoricalFailureCoverageArtifact,
        HistoricalFailureCoverageV1, HistoricalFailureEvidenceArtifact,
        HistoricalFailureObligationV1, HistoricalFailureRecordArtifact,
        HistoricalFailureRecordInput, HistoricalFailureRecordV1, HistoricalFailureScope,
        HistoricalObservedFailureArtifact, HistoricalReproductionArtifact,
        HistoricalValidationStage, MigrationDomainFamilyName, OracleFailureMechanismName,
        TargetMechanismName,
    };

    fn id<T: ContentType>(seed: &str) -> ContentId<T> {
        ContentId::derive(seed.as_bytes()).expect("identity")
    }

    fn evidence(seeds: &[&str]) -> Vec<ContentId<HistoricalFailureEvidenceArtifact>> {
        let mut values: Vec<_> = seeds
            .iter()
            .map(|seed| id::<HistoricalFailureEvidenceArtifact>(seed))
            .collect();
        values.sort_by_key(ContentId::to_wire);
        values
    }

    fn target_record(class: &str, mechanism: &str) -> HistoricalFailureRecordV1 {
        HistoricalFailureRecordV1::new(HistoricalFailureRecordInput {
            failure_class: HistoricalFailureClassName::new(class).expect("class"),
            domain_family: MigrationDomainFamilyName::new("reduction").expect("family"),
            scope: HistoricalFailureScope::Target {
                mechanisms: vec![TargetMechanismName::new(mechanism).expect("mechanism")],
            },
            observed_stage: HistoricalValidationStage::TargetCompilation,
            source_evidence: evidence(&["issue-record", "compiler-log"]),
            observed_failure: id::<HistoricalObservedFailureArtifact>("compile-failure"),
            reproduction_fixture: id::<HistoricalReproductionArtifact>("compile-reproducer"),
            license_provenance: id::<LicenseProvenanceArtifact>("project-license"),
        })
        .expect("record")
    }

    fn compile_obligation(record: &HistoricalFailureRecordV1) -> HistoricalFailureObligationV1 {
        HistoricalFailureObligationV1::from_record(
            record,
            HistoricalDetectionRequirement::CompileDiagnostic {
                diagnostic_class: HistoricalDiagnosticClassName::new("address-space-type-error")
                    .expect("diagnostic"),
            },
        )
        .expect("obligation")
    }

    #[test]
    fn exact_records_bind_provenance_detection_and_coverage_identity() {
        let data_copy = target_record("data-copy-parameter-layout", "data-copy-ext-params");
        let gm_addr = target_record("gm-addr-address-space", "gm-addr");
        let data_copy_obligation = compile_obligation(&data_copy);
        let gm_addr_obligation = compile_obligation(&gm_addr);
        data_copy_obligation
            .validate_record(&data_copy)
            .expect("record binding");
        gm_addr_obligation
            .validate_record(&gm_addr)
            .expect("record binding");

        let coverage = HistoricalFailureCoverageV1::new(
            id::<CallerDomainBodyArtifact>("reduction-domain"),
            MigrationDomainFamilyName::new("reduction").expect("family"),
            vec![data_copy_obligation, gm_addr_obligation],
        )
        .expect("coverage");
        let bytes = cairn_codec::to_vec(&coverage).expect("bytes");
        assert_eq!(
            cairn_codec::from_slice::<HistoricalFailureCoverageV1>(&bytes).expect("round trip"),
            coverage
        );
        let coverage_id =
            ContentId::<HistoricalFailureCoverageArtifact>::derive(&bytes).expect("coverage id");

        let changed_record = target_record("gm-addr-address-space-v2", "gm-addr");
        let changed = HistoricalFailureCoverageV1::new(
            id::<CallerDomainBodyArtifact>("reduction-domain"),
            MigrationDomainFamilyName::new("reduction").expect("family"),
            vec![
                compile_obligation(&data_copy),
                compile_obligation(&changed_record),
            ],
        )
        .expect("changed coverage");
        let changed_id = ContentId::<HistoricalFailureCoverageArtifact>::derive(
            &cairn_codec::to_vec(&changed).expect("changed bytes"),
        )
        .expect("changed id");
        assert_ne!(coverage_id, changed_id);
    }

    #[test]
    fn oracle_and_target_scope_stage_and_detector_mismatches_fail_closed() {
        let oracle_record = HistoricalFailureRecordV1::new(HistoricalFailureRecordInput {
            failure_class: HistoricalFailureClassName::new("single-order-false-reject")
                .expect("class"),
            domain_family: MigrationDomainFamilyName::new("reduction").expect("family"),
            scope: HistoricalFailureScope::Oracle {
                mechanism: OracleFailureMechanismName::new("single-evaluation-order")
                    .expect("mechanism"),
            },
            observed_stage: HistoricalValidationStage::OracleComparison,
            source_evidence: evidence(&["historical-verdict"]),
            observed_failure: id::<HistoricalObservedFailureArtifact>("false-reject"),
            reproduction_fixture: id::<HistoricalReproductionArtifact>("single-order-fixture"),
            license_provenance: id::<LicenseProvenanceArtifact>("project-license"),
        })
        .expect("oracle record");
        HistoricalFailureObligationV1::from_record(
            &oracle_record,
            HistoricalDetectionRequirement::OracleVerdictDivergence,
        )
        .expect("oracle obligation");
        assert!(matches!(
            HistoricalFailureObligationV1::from_record(
                &oracle_record,
                HistoricalDetectionRequirement::AnyInvocationFailure,
            ),
            Err(HistoricalFailureContractError::DetectionStageMismatch)
        ));

        let wrong_scope = HistoricalFailureRecordV1::new(HistoricalFailureRecordInput {
            failure_class: HistoricalFailureClassName::new("gm-addr-address-space").expect("class"),
            domain_family: MigrationDomainFamilyName::new("reduction").expect("family"),
            scope: HistoricalFailureScope::Target {
                mechanisms: vec![TargetMechanismName::new("gm-addr").expect("mechanism")],
            },
            observed_stage: HistoricalValidationStage::OracleComparison,
            source_evidence: evidence(&["issue"]),
            observed_failure: id::<HistoricalObservedFailureArtifact>("failure"),
            reproduction_fixture: id::<HistoricalReproductionArtifact>("fixture"),
            license_provenance: id::<LicenseProvenanceArtifact>("license"),
        });
        assert!(matches!(
            wrong_scope,
            Err(HistoricalFailureContractError::ScopeStageMismatch)
        ));
    }

    #[test]
    fn missing_noncanonical_or_cross_family_provenance_is_rejected() {
        assert!(HistoricalFailureClassName::new("GM_ADDR").is_err());
        let empty_evidence = HistoricalFailureRecordV1::new(HistoricalFailureRecordInput {
            failure_class: HistoricalFailureClassName::new("gm-addr-address-space").expect("class"),
            domain_family: MigrationDomainFamilyName::new("reduction").expect("family"),
            scope: HistoricalFailureScope::Target {
                mechanisms: vec![TargetMechanismName::new("gm-addr").expect("mechanism")],
            },
            observed_stage: HistoricalValidationStage::TargetCompilation,
            source_evidence: Vec::new(),
            observed_failure: id::<HistoricalObservedFailureArtifact>("failure"),
            reproduction_fixture: id::<HistoricalReproductionArtifact>("fixture"),
            license_provenance: id::<LicenseProvenanceArtifact>("license"),
        });
        assert!(matches!(
            empty_evidence,
            Err(HistoricalFailureContractError::EmptySet { .. })
        ));

        let mut duplicated = evidence(&["same"]);
        duplicated.push(duplicated[0]);
        let duplicate_evidence = HistoricalFailureRecordV1::new(HistoricalFailureRecordInput {
            failure_class: HistoricalFailureClassName::new("gm-addr-address-space").expect("class"),
            domain_family: MigrationDomainFamilyName::new("reduction").expect("family"),
            scope: HistoricalFailureScope::Target {
                mechanisms: vec![TargetMechanismName::new("gm-addr").expect("mechanism")],
            },
            observed_stage: HistoricalValidationStage::TargetCompilation,
            source_evidence: duplicated,
            observed_failure: id::<HistoricalObservedFailureArtifact>("failure"),
            reproduction_fixture: id::<HistoricalReproductionArtifact>("fixture"),
            license_provenance: id::<LicenseProvenanceArtifact>("license"),
        });
        assert!(matches!(
            duplicate_evidence,
            Err(HistoricalFailureContractError::NonCanonicalSet { .. })
        ));

        let record = target_record("gm-addr-address-space", "gm-addr");
        let obligation = compile_obligation(&record);
        assert!(matches!(
            HistoricalFailureCoverageV1::new(
                id::<CallerDomainBodyArtifact>("domain"),
                MigrationDomainFamilyName::new("elementwise").expect("family"),
                vec![obligation],
            ),
            Err(HistoricalFailureContractError::DomainFamilyMismatch)
        ));
    }

    #[test]
    fn record_binding_and_strict_v1_survive_persistence_attacks() {
        let record = target_record("gm-addr-address-space", "gm-addr");
        let other_record = target_record("data-copy-parameter-layout", "data-copy-ext-params");
        let obligation = compile_obligation(&record);
        assert!(matches!(
            obligation.validate_record(&other_record),
            Err(HistoricalFailureContractError::RecordIdentityMismatch)
        ));

        let bytes = cairn_codec::to_vec(&obligation).expect("bytes");
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        value["failure_class"] = serde_json::json!("forged-class");
        let forged: HistoricalFailureObligationV1 =
            serde_json::from_value(value).expect("locally valid forged metadata");
        assert!(matches!(
            forged.validate_record(&record),
            Err(HistoricalFailureContractError::RecordMetadataMismatch)
        ));

        let coverage = HistoricalFailureCoverageV1::new(
            id::<CallerDomainBodyArtifact>("domain"),
            MigrationDomainFamilyName::new("reduction").expect("family"),
            vec![obligation],
        )
        .expect("coverage");
        let bytes = cairn_codec::to_vec(&coverage).expect("coverage bytes");
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        value["schema_version"] = serde_json::json!(2);
        assert!(serde_json::from_value::<HistoricalFailureCoverageV1>(value.clone()).is_err());
        value["schema_version"] = serde_json::json!(1);
        value["legacy_issue_url"] = serde_json::json!("https://example.invalid/issue");
        assert!(serde_json::from_value::<HistoricalFailureCoverageV1>(value).is_err());

        let record_bytes = cairn_codec::to_vec(&record).expect("record bytes");
        let record_id =
            ContentId::<HistoricalFailureRecordArtifact>::derive(&record_bytes).expect("record id");
        assert_eq!(record_id, forged.record());
    }

    #[test]
    fn obligation_sets_must_be_nonempty_unique_and_ordered() {
        let domain = id::<CallerDomainBodyArtifact>("domain");
        let family = MigrationDomainFamilyName::new("reduction").expect("family");
        assert!(matches!(
            HistoricalFailureCoverageV1::new(domain, family.clone(), Vec::new()),
            Err(HistoricalFailureContractError::EmptySet { .. })
        ));
        let record = target_record("gm-addr-address-space", "gm-addr");
        let obligation = compile_obligation(&record);
        assert!(matches!(
            HistoricalFailureCoverageV1::new(domain, family, vec![obligation.clone(), obligation],),
            Err(HistoricalFailureContractError::NonCanonicalSet { .. })
        ));
    }
}
