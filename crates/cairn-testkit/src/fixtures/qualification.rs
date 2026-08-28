use std::{collections::BTreeSet, fmt, fs, path::Path, str::FromStr};

use cairn_protocol::{ContentId, ContentType, IdentityError};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

use super::{
    DevelopmentSliceId, FixtureAuthorId, FixtureLicense, GitCommitId, IntentBundleIdentity,
    PublicDataClassification, RepositoryPath, RestrictedReviewReceiptId,
};

const QUALIFICATION_ROOT: &str = "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/";
const MAX_LABEL_LEN: usize = 128;
const DEV001_BUNDLE: &str = "cairn:v1:sha256:testkit.intent-public-bundle.v1:fa2eb4064e772775e886e4feb2f39ca330d8988b7b5227fa6af2f497b7b488fc";
const DEV001_REVIEW_RECEIPT: &str = "cairn:v1:sha256:testkit.restricted-review-receipt.v1:746b5bb5a718d3508311ec7b596299f4c30df2fe04a57a1d77bccb9e6553028e";

/// Strict current-V1 validation failure for the D-040 qualification-control bundle.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum QualificationFixtureError {
    #[error("qualification fixture schema version must be 1")]
    UnsupportedSchemaVersion,
    #[error("{kind} is not canonical")]
    InvalidValue { kind: &'static str },
    #[error("{field} must contain the exact canonical required set")]
    NonCanonicalSet { field: &'static str },
    #[error("qualification fixture fields are inconsistent")]
    InconsistentFixture,
    #[error("declared {kind} identity does not match exact bytes")]
    IdentityMismatch { kind: &'static str },
    #[error("qualification fixture codec error: {message}")]
    Codec { message: String },
    #[error("qualification fixture identity error: {message}")]
    Identity { message: String },
    #[error("qualification fixture I/O error: {message}")]
    Io { message: String },
}

impl From<IdentityError> for QualificationFixtureError {
    fn from(error: IdentityError) -> Self {
        Self::Identity {
            message: error.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SchemaV1;

impl Serialize for SchemaV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u16(1)
    }
}

impl<'de> Deserialize<'de> for SchemaV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match u16::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(de::Error::custom(
                QualificationFixtureError::UnsupportedSchemaVersion,
            )),
        }
    }
}

fn validate_label(value: &str, kind: &'static str) -> Result<(), QualificationFixtureError> {
    if value.is_empty()
        || value.len() > MAX_LABEL_LEN
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.' | b'_')
        })
    {
        return Err(QualificationFixtureError::InvalidValue { kind });
    }
    Ok(())
}

macro_rules! validated_label {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a canonical semantic label.
            ///
            /// # Errors
            ///
            /// Rejects empty, oversized, or non-canonical labels.
            pub fn new(value: impl Into<String>) -> Result<Self, QualificationFixtureError> {
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

validated_label!(
    /// Stable identity of one independently authored qualification control case.
    QualificationControlCaseId,
    "qualification control case"
);
validated_label!(
    /// Identity of the role that authors qualification controls, distinct from reviewers.
    QualificationControlAuthorId,
    "qualification control author"
);

/// Identity of a non-author qualification-control reviewer.
///
/// ```compile_fail
/// use cairn_testkit::fixtures::{QualificationControlAuthorId, QualificationControlReviewerId};
/// fn require_reviewer(_: QualificationControlReviewerId) {}
/// let author: QualificationControlAuthorId = todo!();
/// require_reviewer(author);
/// ```
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct QualificationControlReviewerId(String);

impl QualificationControlReviewerId {
    /// Creates an identity in the qualification-reviewer namespace.
    ///
    /// # Errors
    ///
    /// Rejects non-canonical identities outside `qualification-reviewer-*`.
    pub fn new(value: impl Into<String>) -> Result<Self, QualificationFixtureError> {
        let value = value.into();
        validate_label(&value, "qualification control reviewer")?;
        if !value.starts_with("qualification-reviewer-") {
            return Err(QualificationFixtureError::InvalidValue {
                kind: "qualification control reviewer",
            });
        }
        Ok(Self(value))
    }

    /// Returns the canonical reviewer identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for QualificationControlReviewerId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Public repository path owned by the D-040 qualification bundle.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct IntentQualificationArtifactPath(String);

impl IntentQualificationArtifactPath {
    /// Validates an exact public qualification-bundle path.
    ///
    /// # Errors
    ///
    /// Rejects paths outside the public root or containing private/traversal components.
    pub fn new(value: impl Into<String>) -> Result<Self, QualificationFixtureError> {
        let value = value.into();
        let path = Path::new(&value);
        if !value.starts_with(QUALIFICATION_ROOT)
            || value.contains('\\')
            || path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::CurDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
            || value
                .split('/')
                .any(|part| matches!(part, ".cairn" | "secrets" | "restricted"))
        {
            return Err(QualificationFixtureError::InvalidValue {
                kind: "public qualification artifact path",
            });
        }
        Ok(Self(value))
    }

    /// Returns the repository-relative path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for IntentQualificationArtifactPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

pub enum IntentQualificationPublicArtifact {}
impl ContentType for IntentQualificationPublicArtifact {
    const DOMAIN: &'static str = "testkit.intent-qualification-public-artifact.v1";
}

pub enum IntentQualificationPublicBundle {}
impl ContentType for IntentQualificationPublicBundle {
    const DOMAIN: &'static str = "testkit.intent-qualification-public-bundle.v1";
}

pub enum IntentQualificationReviewSubject {}
impl ContentType for IntentQualificationReviewSubject {
    const DOMAIN: &'static str = "testkit.intent-qualification-review-subject.v1";
}

pub enum QualificationControlArtifact {}
impl ContentType for QualificationControlArtifact {
    const DOMAIN: &'static str = "testkit.intent-qualification-control.v1";
}

pub enum RestrictedQualificationControl {}
impl ContentType for RestrictedQualificationControl {
    const DOMAIN: &'static str = "testkit.restricted-qualification-control.v1";
}

pub enum RestrictedQualificationManifest {}
impl ContentType for RestrictedQualificationManifest {
    const DOMAIN: &'static str = "testkit.restricted-qualification-manifest.v1";
}

pub enum QualificationControlReviewReceipt {}
impl ContentType for QualificationControlReviewReceipt {
    const DOMAIN: &'static str = "testkit.qualification-control-review-receipt.v1";
}

macro_rules! typed_identity {
    ($(#[$meta:meta])* $name:ident, $artifact:ty, $kind:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Eq, Hash, PartialEq)]
        pub struct $name(ContentId<$artifact>);

        impl $name {
            /// Derives the identity from exact bytes in its semantic domain.
            ///
            /// # Errors
            ///
            /// Returns an error when the content identity frame is invalid.
            pub fn derive(bytes: &[u8]) -> Result<Self, QualificationFixtureError> {
                Ok(Self(ContentId::derive(bytes)?))
            }

            /// Returns the tagged wire identity.
            #[must_use]
            pub fn to_wire(self) -> String {
                self.0.to_wire()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.debug_tuple(stringify!($name)).field(&self.to_wire()).finish()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.to_wire())
            }
        }

        impl FromStr for $name {
            type Err = IdentityError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value.parse().map(Self)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.to_wire())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                String::deserialize(deserializer)?
                    .parse()
                    .map_err(|error: IdentityError| de::Error::custom(format!("invalid {}: {error}", $kind)))
            }
        }
    };
}

typed_identity!(
    /// Exact identity of one public qualification artifact.
    ///
    /// ```compile_fail
    /// use cairn_testkit::fixtures::{IntentQualificationArtifactIdentity, IntentQualificationBundleIdentity};
    /// fn require_artifact(_: IntentQualificationArtifactIdentity) {}
    /// let bundle: IntentQualificationBundleIdentity = todo!();
    /// require_artifact(bundle);
    /// ```
    IntentQualificationArtifactIdentity,
    IntentQualificationPublicArtifact,
    "qualification artifact identity"
);
typed_identity!(
    /// Exact identity of the final public qualification manifest bytes.
    IntentQualificationBundleIdentity,
    IntentQualificationPublicBundle,
    "qualification bundle identity"
);
typed_identity!(
    /// Exact pre-receipt public manifest bytes reviewed before freezing.
    IntentQualificationReviewSubjectIdentity,
    IntentQualificationReviewSubject,
    "qualification review subject identity"
);
typed_identity!(
    /// Semantic identity of one independently authored control suite.
    QualificationControlIdentity,
    QualificationControlArtifact,
    "qualification control identity"
);
typed_identity!(
    /// Exact private qualification-control identity.
    RestrictedQualificationControlId,
    RestrictedQualificationControl,
    "restricted qualification control identity"
);
typed_identity!(
    /// Exact private qualification-control manifest identity.
    RestrictedQualificationManifestId,
    RestrictedQualificationManifest,
    "restricted qualification manifest identity"
);
typed_identity!(
    /// Redacted identity of an independent control-review receipt.
    ///
    /// ```compile_fail
    /// use cairn_testkit::fixtures::{QualificationControlReviewReceiptId, RestrictedReviewReceiptId};
    /// fn require_control_review(_: QualificationControlReviewReceiptId) {}
    /// let corpus_review: RestrictedReviewReceiptId = todo!();
    /// require_control_review(corpus_review);
    /// ```
    QualificationControlReviewReceiptId,
    QualificationControlReviewReceipt,
    "qualification control review receipt identity"
);

/// The exact ten D-040 semantic mechanism slots.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualificationMechanismSlot {
    StrictV1Identity,
    AbiStaticFacts,
    RequiredIntentEvidence,
    RecipePlanValidation,
    RecordedHostRunner,
    RawObservationComparison,
    ReceiptClosure,
    IntentPolicy,
    IntentGate,
    DiagnosticRedaction,
}

impl QualificationMechanismSlot {
    /// Returns the canonical ten-slot order fixed by D-040.
    #[must_use]
    pub const fn all() -> [Self; 10] {
        [
            Self::StrictV1Identity,
            Self::AbiStaticFacts,
            Self::RequiredIntentEvidence,
            Self::RecipePlanValidation,
            Self::RecordedHostRunner,
            Self::RawObservationComparison,
            Self::ReceiptClosure,
            Self::IntentPolicy,
            Self::IntentGate,
            Self::DiagnosticRedaction,
        ]
    }

    fn owner_slice(self) -> &'static str {
        match self {
            Self::StrictV1Identity => "dev-100",
            Self::AbiStaticFacts => "dev-102",
            Self::RequiredIntentEvidence | Self::RecipePlanValidation => "dev-103",
            Self::RecordedHostRunner
            | Self::RawObservationComparison
            | Self::ReceiptClosure
            | Self::IntentPolicy
            | Self::IntentGate
            | Self::DiagnosticRedaction => "dev-104",
        }
    }

    fn expected_scope(self) -> MechanismScope {
        match self {
            Self::StrictV1Identity => MechanismScope::CurrentV1FixtureBoundary,
            Self::AbiStaticFacts => MechanismScope::D039CudaHostStaticFacts,
            Self::RequiredIntentEvidence => MechanismScope::D039RequiredIntentObligations,
            Self::RecipePlanValidation => MechanismScope::DeterministicIntentRecipe,
            Self::RecordedHostRunner => MechanismScope::RecordedAndRealHostObservation,
            Self::RawObservationComparison => MechanismScope::TypedRawObservationRelation,
            Self::ReceiptClosure => MechanismScope::IntentReceiptGraphClosure,
            Self::IntentPolicy => MechanismScope::D039ClaimScopedDisposition,
            Self::IntentGate => MechanismScope::IntentAdmissionConstruction,
            Self::DiagnosticRedaction => MechanismScope::IntentPublicDiagnosticProjection,
        }
    }

    fn expected_deadline(self) -> QualificationDeadline {
        match self {
            Self::StrictV1Identity
            | Self::AbiStaticFacts
            | Self::RequiredIntentEvidence
            | Self::RecipePlanValidation => QualificationDeadline::OwnerSliceAcceptance,
            Self::RecordedHostRunner
            | Self::RawObservationComparison
            | Self::ReceiptClosure
            | Self::IntentPolicy
            | Self::DiagnosticRedaction => QualificationDeadline::FirstIntentGateUse,
            Self::IntentGate => QualificationDeadline::FirstIntentOutcome,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum MechanismScope {
    CurrentV1FixtureBoundary,
    D039CudaHostStaticFacts,
    D039RequiredIntentObligations,
    DeterministicIntentRecipe,
    RecordedAndRealHostObservation,
    TypedRawObservationRelation,
    IntentReceiptGraphClosure,
    D039ClaimScopedDisposition,
    IntentAdmissionConstruction,
    IntentPublicDiagnosticProjection,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum QualificationDeadline {
    #[serde(rename = "before-owner-slice-acceptance")]
    OwnerSliceAcceptance,
    #[serde(rename = "before-first-intent-gate-use")]
    FirstIntentGateUse,
    #[serde(rename = "before-first-intent-outcome")]
    FirstIntentOutcome,
}

/// Qualification-control evidence categories, distinct from mechanism outcomes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualificationControlCategory {
    Honest,
    FalseAccept,
    FalseReject,
    Conflict,
    Unknown,
    WrongBinding,
    Missing,
    Duplicate,
    Tampered,
    Fault,
    RedactionCanary,
    OverRedaction,
    ConstructorBypass,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum MechanismLimitation {
    NoIntentSemanticAuthority,
    NoRuntimeObservation,
    DoesNotSatisfyObligations,
    NoExternalEffect,
    NoCudaOrAscendDeviceClaim,
    NoClaimAdjudication,
    NoSemanticInterpretation,
    ClaimDomainScopedOnly,
    RequiresQualifiedClosure,
    ProjectionOnly,
}

fn expected_limitations(slot: QualificationMechanismSlot) -> &'static [MechanismLimitation] {
    use MechanismLimitation as L;
    match slot {
        QualificationMechanismSlot::StrictV1Identity => &[L::NoIntentSemanticAuthority],
        QualificationMechanismSlot::AbiStaticFacts => &[L::NoRuntimeObservation],
        QualificationMechanismSlot::RequiredIntentEvidence => &[L::DoesNotSatisfyObligations],
        QualificationMechanismSlot::RecipePlanValidation => &[L::NoExternalEffect],
        QualificationMechanismSlot::RecordedHostRunner => &[L::NoCudaOrAscendDeviceClaim],
        QualificationMechanismSlot::RawObservationComparison => &[L::NoClaimAdjudication],
        QualificationMechanismSlot::ReceiptClosure => &[L::NoSemanticInterpretation],
        QualificationMechanismSlot::IntentPolicy => &[L::ClaimDomainScopedOnly],
        QualificationMechanismSlot::IntentGate => &[L::RequiresQualifiedClosure],
        QualificationMechanismSlot::DiagnosticRedaction => &[L::ProjectionOnly],
    }
}

fn required_categories(
    slot: QualificationMechanismSlot,
) -> &'static [QualificationControlCategory] {
    use QualificationControlCategory as C;
    match slot {
        QualificationMechanismSlot::StrictV1Identity
        | QualificationMechanismSlot::AbiStaticFacts
        | QualificationMechanismSlot::RequiredIntentEvidence
        | QualificationMechanismSlot::RawObservationComparison => {
            &[C::Honest, C::FalseAccept, C::FalseReject]
        }
        QualificationMechanismSlot::RecipePlanValidation => {
            &[C::Honest, C::FalseAccept, C::WrongBinding, C::Missing]
        }
        QualificationMechanismSlot::RecordedHostRunner => {
            &[C::Honest, C::FalseAccept, C::FalseReject, C::Fault]
        }
        QualificationMechanismSlot::ReceiptClosure => &[
            C::Honest,
            C::WrongBinding,
            C::Missing,
            C::Duplicate,
            C::Tampered,
        ],
        QualificationMechanismSlot::IntentPolicy => &[
            C::Honest,
            C::FalseAccept,
            C::FalseReject,
            C::Conflict,
            C::Unknown,
        ],
        QualificationMechanismSlot::IntentGate => &[
            C::Honest,
            C::FalseAccept,
            C::Conflict,
            C::Unknown,
            C::ConstructorBypass,
        ],
        QualificationMechanismSlot::DiagnosticRedaction => {
            &[C::Honest, C::RedactionCanary, C::OverRedaction]
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct MechanismContractV1 {
    slot: QualificationMechanismSlot,
    owner_slice: DevelopmentSliceId,
    scope: MechanismScope,
    limitations: Vec<MechanismLimitation>,
    required_control_categories: Vec<QualificationControlCategory>,
    qualification_deadline: QualificationDeadline,
}

impl MechanismContractV1 {
    fn validate(&self) -> Result<(), QualificationFixtureError> {
        if self.owner_slice.as_str() != self.slot.owner_slice()
            || self.scope != self.slot.expected_scope()
            || self.limitations != expected_limitations(self.slot)
            || self.required_control_categories != required_categories(self.slot)
            || self.qualification_deadline != self.slot.expected_deadline()
        {
            return Err(QualificationFixtureError::InconsistentFixture);
        }
        Ok(())
    }
}

/// Strict current-V1 D-040 mechanism-contract set. It contains no implementation or receipt field.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentMechanismQualificationContractSetV1 {
    schema_version: SchemaV1,
    contracts: Vec<MechanismContractV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentMechanismQualificationContractSetWire {
    schema_version: SchemaV1,
    contracts: Vec<MechanismContractV1>,
}

impl IntentMechanismQualificationContractSetV1 {
    fn validate(&self) -> Result<(), QualificationFixtureError> {
        if self.contracts.len() != QualificationMechanismSlot::all().len() {
            return Err(QualificationFixtureError::NonCanonicalSet {
                field: "mechanism contracts",
            });
        }
        for (contract, slot) in self.contracts.iter().zip(QualificationMechanismSlot::all()) {
            contract.validate()?;
            if contract.slot != slot {
                return Err(QualificationFixtureError::NonCanonicalSet {
                    field: "mechanism contracts",
                });
            }
        }
        Ok(())
    }

    /// Returns the exact D-040 slot count.
    #[must_use]
    pub fn contract_count(&self) -> usize {
        self.contracts.len()
    }

    /// Returns required categories for a slot, for cross-artifact closure tests.
    #[must_use]
    pub fn required_categories_for(
        &self,
        slot: QualificationMechanismSlot,
    ) -> &[QualificationControlCategory] {
        self.contracts
            .iter()
            .find(|contract| contract.slot == slot)
            .map_or(&[], |contract| &contract.required_control_categories)
    }
}

impl TryFrom<IntentMechanismQualificationContractSetWire>
    for IntentMechanismQualificationContractSetV1
{
    type Error = QualificationFixtureError;

    fn try_from(wire: IntentMechanismQualificationContractSetWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            contracts: wire.contracts,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for IntentMechanismQualificationContractSetV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentMechanismQualificationContractSetWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Workstream review roles used only in development qualification fixtures.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualificationReviewRole {
    WsAdmission,
    WsDomain,
    WsExecution,
    WsQuality,
    WsRecord,
    WsSir,
}

fn owner_role(slot: QualificationMechanismSlot) -> QualificationReviewRole {
    match slot {
        QualificationMechanismSlot::StrictV1Identity
        | QualificationMechanismSlot::IntentPolicy
        | QualificationMechanismSlot::IntentGate => QualificationReviewRole::WsDomain,
        QualificationMechanismSlot::AbiStaticFacts => QualificationReviewRole::WsSir,
        QualificationMechanismSlot::RecordedHostRunner => QualificationReviewRole::WsExecution,
        QualificationMechanismSlot::RequiredIntentEvidence
        | QualificationMechanismSlot::RecipePlanValidation
        | QualificationMechanismSlot::RawObservationComparison
        | QualificationMechanismSlot::ReceiptClosure
        | QualificationMechanismSlot::DiagnosticRedaction => QualificationReviewRole::WsAdmission,
    }
}

fn expected_reviewers(slot: QualificationMechanismSlot) -> &'static [QualificationReviewRole] {
    use QualificationReviewRole as R;
    match slot {
        QualificationMechanismSlot::StrictV1Identity
        | QualificationMechanismSlot::RequiredIntentEvidence
        | QualificationMechanismSlot::RecipePlanValidation
        | QualificationMechanismSlot::RawObservationComparison
        | QualificationMechanismSlot::IntentPolicy
        | QualificationMechanismSlot::IntentGate => &[R::WsAdmission, R::WsDomain, R::WsQuality],
        QualificationMechanismSlot::AbiStaticFacts => &[R::WsAdmission, R::WsQuality, R::WsSir],
        QualificationMechanismSlot::RecordedHostRunner => {
            &[R::WsAdmission, R::WsExecution, R::WsQuality]
        }
        QualificationMechanismSlot::ReceiptClosure
        | QualificationMechanismSlot::DiagnosticRedaction => {
            &[R::WsAdmission, R::WsQuality, R::WsRecord]
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewAssignmentV1 {
    slot: QualificationMechanismSlot,
    mechanism_owner: QualificationReviewRole,
    control_author: QualificationControlAuthorId,
    required_reviewers: Vec<QualificationReviewRole>,
}

/// Exact role-level review assignments for all ten mechanism slots.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentQualificationReviewAssignmentsV1 {
    schema_version: SchemaV1,
    assignments: Vec<ReviewAssignmentV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentQualificationReviewAssignmentsWire {
    schema_version: SchemaV1,
    assignments: Vec<ReviewAssignmentV1>,
}

impl IntentQualificationReviewAssignmentsV1 {
    fn validate(&self) -> Result<(), QualificationFixtureError> {
        if self.assignments.len() != QualificationMechanismSlot::all().len() {
            return Err(QualificationFixtureError::NonCanonicalSet {
                field: "review assignments",
            });
        }
        for (assignment, slot) in self
            .assignments
            .iter()
            .zip(QualificationMechanismSlot::all())
        {
            if assignment.slot != slot
                || assignment.mechanism_owner != owner_role(slot)
                || assignment.control_author.as_str() != "qualification-control-author-ws-quality"
                || assignment.required_reviewers != expected_reviewers(slot)
                || !assignment
                    .required_reviewers
                    .contains(&assignment.mechanism_owner)
                || !assignment
                    .required_reviewers
                    .contains(&QualificationReviewRole::WsAdmission)
                || !assignment
                    .required_reviewers
                    .contains(&QualificationReviewRole::WsQuality)
            {
                return Err(QualificationFixtureError::InconsistentFixture);
            }
        }
        Ok(())
    }

    /// Returns the exact assignment count.
    #[must_use]
    pub fn assignment_count(&self) -> usize {
        self.assignments.len()
    }
}

impl TryFrom<IntentQualificationReviewAssignmentsWire> for IntentQualificationReviewAssignmentsV1 {
    type Error = QualificationFixtureError;

    fn try_from(wire: IntentQualificationReviewAssignmentsWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            assignments: wire.assignments,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for IntentQualificationReviewAssignmentsV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentQualificationReviewAssignmentsWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Verdict-relevant changes that invalidate a future mechanism qualification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequalificationTrigger {
    Source,
    Policy,
    Dependency,
    Toolchain,
    CalibrationEnvironment,
    Limitation,
    Schema,
    InputContract,
    ExposurePolicy,
}

fn expected_triggers(slot: QualificationMechanismSlot) -> &'static [RequalificationTrigger] {
    use RequalificationTrigger as T;
    match slot {
        QualificationMechanismSlot::StrictV1Identity => {
            &[T::Source, T::Dependency, T::Limitation, T::Schema]
        }
        QualificationMechanismSlot::AbiStaticFacts => &[
            T::Source,
            T::Dependency,
            T::Toolchain,
            T::Limitation,
            T::InputContract,
        ],
        QualificationMechanismSlot::RequiredIntentEvidence
        | QualificationMechanismSlot::IntentPolicy
        | QualificationMechanismSlot::IntentGate => &[
            T::Source,
            T::Policy,
            T::Dependency,
            T::Limitation,
            T::InputContract,
        ],
        QualificationMechanismSlot::RecipePlanValidation => &[
            T::Source,
            T::Policy,
            T::Dependency,
            T::Limitation,
            T::InputContract,
            T::ExposurePolicy,
        ],
        QualificationMechanismSlot::RecordedHostRunner => &[
            T::Source,
            T::Dependency,
            T::Toolchain,
            T::CalibrationEnvironment,
            T::Limitation,
            T::InputContract,
        ],
        QualificationMechanismSlot::RawObservationComparison
        | QualificationMechanismSlot::ReceiptClosure => &[
            T::Source,
            T::Dependency,
            T::Limitation,
            T::Schema,
            T::InputContract,
        ],
        QualificationMechanismSlot::DiagnosticRedaction => &[
            T::Source,
            T::Policy,
            T::Dependency,
            T::Limitation,
            T::Schema,
            T::ExposurePolicy,
        ],
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RefutationEffect {
    BlockUseAndRequireReverseImpact,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RequalificationPlanV1 {
    slot: QualificationMechanismSlot,
    triggers: Vec<RequalificationTrigger>,
    refutation_effect: RefutationEffect,
}

/// Exact requalification and refutation plans for all ten mechanism slots.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentRequalificationPlansV1 {
    schema_version: SchemaV1,
    plans: Vec<RequalificationPlanV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentRequalificationPlansWire {
    schema_version: SchemaV1,
    plans: Vec<RequalificationPlanV1>,
}

impl IntentRequalificationPlansV1 {
    fn validate(&self) -> Result<(), QualificationFixtureError> {
        if self.plans.len() != QualificationMechanismSlot::all().len() {
            return Err(QualificationFixtureError::NonCanonicalSet {
                field: "requalification plans",
            });
        }
        for (plan, slot) in self.plans.iter().zip(QualificationMechanismSlot::all()) {
            if plan.slot != slot
                || plan.triggers != expected_triggers(slot)
                || plan.refutation_effect != RefutationEffect::BlockUseAndRequireReverseImpact
            {
                return Err(QualificationFixtureError::InconsistentFixture);
            }
        }
        Ok(())
    }

    /// Returns the exact plan count.
    #[must_use]
    pub fn plan_count(&self) -> usize {
        self.plans.len()
    }
}

impl TryFrom<IntentRequalificationPlansWire> for IntentRequalificationPlansV1 {
    type Error = QualificationFixtureError;

    fn try_from(wire: IntentRequalificationPlansWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            plans: wire.plans,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for IntentRequalificationPlansV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentRequalificationPlansWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Exact, mechanism-specific stimuli used by the first qualification-control set.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualificationControlStimulus {
    CanonicalV1,
    CanonicalBoundaryV1,
    NonV1,
    UnknownField,
    ConstructorInvariantViolation,
    IdentityMutation,
    ExactD039Abi,
    EquivalentAbiSurface,
    BufferDirectionMutation,
    AbiArityMutation,
    AliasPermissionMutation,
    LaunchBoundMutation,
    CompleteRequiredSet,
    ValidSupplementalEvidence,
    MissingObligation,
    DuplicateObligation,
    ApplicantDeletesObligation,
    WrongClaimScope,
    ValidDeterministicRecipe,
    WrongExperimentKind,
    WrongEnvironmentBinding,
    ExcessCapability,
    MissingPredecessor,
    PlannerSuppliesExpectedAnswer,
    RecordedHostObservation,
    RunnerNoLaunch,
    RunnerStaleOutput,
    RunnerCaptureFailure,
    RunnerTruncatedOutput,
    RunnerCrashRestart,
    ExactRawObservation,
    MissingRawOutput,
    ExtraRawOutput,
    MalformedRawOutput,
    SignedZeroRawDifference,
    CompleteReceiptClosure,
    WrongReceiptBinding,
    MissingReceipt,
    DuplicateReceipt,
    TamperedReceipt,
    StaleReceipt,
    HonestIntentEvidence,
    ConflictingIntentEvidence,
    InsufficientIntentEvidence,
    ImplementationArtifactEvidence,
    SourceDefectEvidence,
    DeploymentQuirkEvidence,
    UserDecisionEvidence,
    CompleteSatisfiedGateInput,
    MissingQualification,
    RefutedMechanism,
    UnclosedObligation,
    StoredPassOnly,
    ConflictGateInput,
    UnknownGateInput,
    ConstructorBypassAttempt,
    PublicDiagnosticAllowlist,
    HiddenCanaryReference,
    SecretCanaryReference,
    PrivateIdentityReference,
    OverRedactedPublicOutcome,
    DistinguishingDisclosure,
}

fn required_stimuli(slot: QualificationMechanismSlot) -> &'static [QualificationControlStimulus] {
    use QualificationControlStimulus as S;
    match slot {
        QualificationMechanismSlot::StrictV1Identity => &[
            S::CanonicalV1,
            S::CanonicalBoundaryV1,
            S::NonV1,
            S::UnknownField,
            S::ConstructorInvariantViolation,
            S::IdentityMutation,
        ],
        QualificationMechanismSlot::AbiStaticFacts => &[
            S::ExactD039Abi,
            S::EquivalentAbiSurface,
            S::BufferDirectionMutation,
            S::AbiArityMutation,
            S::AliasPermissionMutation,
            S::LaunchBoundMutation,
        ],
        QualificationMechanismSlot::RequiredIntentEvidence => &[
            S::CompleteRequiredSet,
            S::ValidSupplementalEvidence,
            S::MissingObligation,
            S::DuplicateObligation,
            S::ApplicantDeletesObligation,
            S::WrongClaimScope,
        ],
        QualificationMechanismSlot::RecipePlanValidation => &[
            S::ValidDeterministicRecipe,
            S::WrongExperimentKind,
            S::WrongEnvironmentBinding,
            S::ExcessCapability,
            S::MissingPredecessor,
            S::PlannerSuppliesExpectedAnswer,
        ],
        QualificationMechanismSlot::RecordedHostRunner => &[
            S::RecordedHostObservation,
            S::RunnerNoLaunch,
            S::RunnerStaleOutput,
            S::RunnerCaptureFailure,
            S::RunnerTruncatedOutput,
            S::RunnerCrashRestart,
        ],
        QualificationMechanismSlot::RawObservationComparison => &[
            S::ExactRawObservation,
            S::MissingRawOutput,
            S::ExtraRawOutput,
            S::MalformedRawOutput,
            S::SignedZeroRawDifference,
        ],
        QualificationMechanismSlot::ReceiptClosure => &[
            S::CompleteReceiptClosure,
            S::WrongReceiptBinding,
            S::MissingReceipt,
            S::DuplicateReceipt,
            S::TamperedReceipt,
            S::StaleReceipt,
        ],
        QualificationMechanismSlot::IntentPolicy => &[
            S::HonestIntentEvidence,
            S::ConflictingIntentEvidence,
            S::InsufficientIntentEvidence,
            S::ImplementationArtifactEvidence,
            S::SourceDefectEvidence,
            S::DeploymentQuirkEvidence,
            S::UserDecisionEvidence,
        ],
        QualificationMechanismSlot::IntentGate => &[
            S::CompleteSatisfiedGateInput,
            S::MissingQualification,
            S::RefutedMechanism,
            S::UnclosedObligation,
            S::StoredPassOnly,
            S::ConflictGateInput,
            S::UnknownGateInput,
            S::ConstructorBypassAttempt,
        ],
        QualificationMechanismSlot::DiagnosticRedaction => &[
            S::PublicDiagnosticAllowlist,
            S::HiddenCanaryReference,
            S::SecretCanaryReference,
            S::PrivateIdentityReference,
            S::OverRedactedPublicOutcome,
            S::DistinguishingDisclosure,
        ],
    }
}

/// Expected control behavior, distinct from a mechanism qualification verdict.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualificationExpectedBehavior {
    ControlAccepted,
    RejectedAtMechanismBoundary,
    RequiredSetDerived,
    PlanValidated,
    NoEffectAuthorized,
    ObservationRecorded,
    NoLaunch,
    StaleOutputRejected,
    CaptureFailed,
    RestartRequired,
    RawRelationRecorded,
    ClosureVerified,
    ClosureRejected,
    IntentEvidenceAccepted,
    Conflict,
    Unknown,
    ImplementationArtifact,
    SourceDefect,
    DeploymentQuirk,
    NeedsUserDecision,
    GateInputComplete,
    AdmissionBlocked,
    DiagnosticPublished,
    DiagnosticRedacted,
    DiagnosticBlocked,
}

fn control_semantics(
    stimulus: QualificationControlStimulus,
) -> (QualificationControlCategory, QualificationExpectedBehavior) {
    use QualificationControlCategory as C;
    use QualificationControlStimulus as S;
    use QualificationExpectedBehavior as E;
    match stimulus {
        S::CanonicalV1 | S::ExactD039Abi | S::ExactRawObservation => {
            (C::Honest, E::ControlAccepted)
        }
        S::CanonicalBoundaryV1 | S::EquivalentAbiSurface => (C::FalseReject, E::ControlAccepted),
        S::NonV1
        | S::UnknownField
        | S::ConstructorInvariantViolation
        | S::IdentityMutation
        | S::BufferDirectionMutation
        | S::AbiArityMutation
        | S::AliasPermissionMutation
        | S::LaunchBoundMutation
        | S::ExtraRawOutput
        | S::MalformedRawOutput
        | S::MissingRawOutput
        | S::WrongClaimScope => (C::FalseAccept, E::RejectedAtMechanismBoundary),
        S::CompleteRequiredSet => (C::Honest, E::RequiredSetDerived),
        S::ValidSupplementalEvidence => (C::FalseReject, E::RequiredSetDerived),
        S::MissingObligation | S::DuplicateObligation | S::ApplicantDeletesObligation => {
            (C::FalseAccept, E::RejectedAtMechanismBoundary)
        }
        S::ValidDeterministicRecipe => (C::Honest, E::PlanValidated),
        S::WrongExperimentKind | S::ExcessCapability | S::PlannerSuppliesExpectedAnswer => {
            (C::FalseAccept, E::NoEffectAuthorized)
        }
        S::WrongEnvironmentBinding => (C::WrongBinding, E::NoEffectAuthorized),
        S::MissingPredecessor => (C::Missing, E::NoEffectAuthorized),
        S::RecordedHostObservation => (C::Honest, E::ObservationRecorded),
        S::RunnerNoLaunch => (C::FalseAccept, E::NoLaunch),
        S::RunnerStaleOutput => (C::FalseAccept, E::StaleOutputRejected),
        S::RunnerCaptureFailure | S::RunnerTruncatedOutput => (C::FalseReject, E::CaptureFailed),
        S::RunnerCrashRestart => (C::Fault, E::RestartRequired),
        S::SignedZeroRawDifference => (C::FalseReject, E::RawRelationRecorded),
        S::CompleteReceiptClosure => (C::Honest, E::ClosureVerified),
        S::WrongReceiptBinding | S::StaleReceipt => (C::WrongBinding, E::ClosureRejected),
        S::MissingReceipt => (C::Missing, E::ClosureRejected),
        S::DuplicateReceipt => (C::Duplicate, E::ClosureRejected),
        S::TamperedReceipt => (C::Tampered, E::ClosureRejected),
        S::HonestIntentEvidence => (C::Honest, E::IntentEvidenceAccepted),
        S::ConflictingIntentEvidence => (C::Conflict, E::Conflict),
        S::InsufficientIntentEvidence => (C::Unknown, E::Unknown),
        S::ImplementationArtifactEvidence => (C::FalseAccept, E::ImplementationArtifact),
        S::SourceDefectEvidence => (C::FalseAccept, E::SourceDefect),
        S::DeploymentQuirkEvidence => (C::FalseAccept, E::DeploymentQuirk),
        S::UserDecisionEvidence => (C::FalseReject, E::NeedsUserDecision),
        S::CompleteSatisfiedGateInput => (C::Honest, E::GateInputComplete),
        S::MissingQualification
        | S::RefutedMechanism
        | S::UnclosedObligation
        | S::StoredPassOnly => (C::FalseAccept, E::AdmissionBlocked),
        S::ConflictGateInput => (C::Conflict, E::AdmissionBlocked),
        S::UnknownGateInput => (C::Unknown, E::AdmissionBlocked),
        S::ConstructorBypassAttempt => (C::ConstructorBypass, E::AdmissionBlocked),
        S::PublicDiagnosticAllowlist => (C::Honest, E::DiagnosticPublished),
        S::HiddenCanaryReference | S::SecretCanaryReference | S::PrivateIdentityReference => {
            (C::RedactionCanary, E::DiagnosticRedacted)
        }
        S::OverRedactedPublicOutcome => (C::OverRedaction, E::DiagnosticPublished),
        S::DistinguishingDisclosure => (C::RedactionCanary, E::DiagnosticBlocked),
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct QualificationControlCaseV1 {
    control_id: QualificationControlCaseId,
    category: QualificationControlCategory,
    stimulus: QualificationControlStimulus,
    expected: QualificationExpectedBehavior,
}

/// One exact independently authored control suite for one D-040 mechanism slot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentQualificationControlSuiteV1 {
    schema_version: SchemaV1,
    slot: QualificationMechanismSlot,
    cases: Vec<QualificationControlCaseV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentQualificationControlSuiteWire {
    schema_version: SchemaV1,
    slot: QualificationMechanismSlot,
    cases: Vec<QualificationControlCaseV1>,
}

impl IntentQualificationControlSuiteV1 {
    fn validate(&self) -> Result<(), QualificationFixtureError> {
        let required = required_stimuli(self.slot);
        if self.cases.len() != required.len() {
            return Err(QualificationFixtureError::NonCanonicalSet {
                field: "qualification control cases",
            });
        }
        let mut ids = BTreeSet::new();
        for (case, stimulus) in self.cases.iter().zip(required) {
            let (category, expected) = control_semantics(*stimulus);
            if case.stimulus != *stimulus
                || case.category != category
                || case.expected != expected
                || !ids.insert(case.control_id.clone())
            {
                return Err(QualificationFixtureError::InconsistentFixture);
            }
        }
        let categories: BTreeSet<_> = self.cases.iter().map(|case| case.category).collect();
        if required_categories(self.slot)
            .iter()
            .any(|required| !categories.contains(required))
        {
            return Err(QualificationFixtureError::NonCanonicalSet {
                field: "qualification control categories",
            });
        }
        Ok(())
    }

    /// Returns the mechanism slot controlled by this suite.
    #[must_use]
    pub const fn slot(&self) -> QualificationMechanismSlot {
        self.slot
    }

    /// Returns the suite's exact case count.
    #[must_use]
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// Returns the distinct control categories covered by the suite.
    #[must_use]
    pub fn categories(&self) -> BTreeSet<QualificationControlCategory> {
        self.cases.iter().map(|case| case.category).collect()
    }

    /// Derives the semantic identity of exact canonical suite bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when the content identity frame is invalid.
    pub fn identity(
        bytes: &[u8],
    ) -> Result<QualificationControlIdentity, QualificationFixtureError> {
        QualificationControlIdentity::derive(bytes)
    }
}

impl TryFrom<IntentQualificationControlSuiteWire> for IntentQualificationControlSuiteV1 {
    type Error = QualificationFixtureError;

    fn try_from(wire: IntentQualificationControlSuiteWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            slot: wire.slot,
            cases: wire.cases,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for IntentQualificationControlSuiteV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentQualificationControlSuiteWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Restricted qualification-control classes. These are not admission-corpus partitions.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestrictedQualificationControlKind {
    WrongBinding,
    HiddenRedactionCanary,
    SecretRedactionCanary,
}

impl RestrictedQualificationControlKind {
    fn all() -> [Self; 3] {
        [
            Self::WrongBinding,
            Self::HiddenRedactionCanary,
            Self::SecretRedactionCanary,
        ]
    }
}

/// Public lifecycle projection for private qualification controls.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestrictedQualificationControlStatus {
    ReviewPending,
    FrozenReviewed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RestrictedQualificationControlSummaryV1 {
    kind: RestrictedQualificationControlKind,
    status: RestrictedQualificationControlStatus,
}

/// Redacted public summary of the private qualification-control set.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentRestrictedQualificationSummaryV1 {
    schema_version: SchemaV1,
    review_receipt_identity: Option<QualificationControlReviewReceiptId>,
    controls: Vec<RestrictedQualificationControlSummaryV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentRestrictedQualificationSummaryWire {
    schema_version: SchemaV1,
    review_receipt_identity: Option<QualificationControlReviewReceiptId>,
    controls: Vec<RestrictedQualificationControlSummaryV1>,
}

impl IntentRestrictedQualificationSummaryV1 {
    fn validate(&self) -> Result<(), QualificationFixtureError> {
        let required = RestrictedQualificationControlKind::all();
        let status = self.controls.first().map(|control| control.status);
        let receipt_is_consistent = matches!(
            (status, self.review_receipt_identity),
            (
                Some(RestrictedQualificationControlStatus::ReviewPending),
                None
            ) | (
                Some(RestrictedQualificationControlStatus::FrozenReviewed),
                Some(_)
            )
        );
        if self.controls.len() != required.len()
            || !receipt_is_consistent
            || !self
                .controls
                .iter()
                .zip(required)
                .all(|(control, kind)| control.kind == kind && Some(control.status) == status)
        {
            return Err(QualificationFixtureError::NonCanonicalSet {
                field: "restricted qualification summaries",
            });
        }
        Ok(())
    }

    /// Returns the exact private control category count without exposing their identities.
    #[must_use]
    pub fn control_count(&self) -> usize {
        self.controls.len()
    }

    /// Reports whether independent private review still blocks freezing.
    #[must_use]
    pub fn is_review_pending(&self) -> bool {
        self.review_receipt_identity.is_none()
            && self.controls.iter().all(|control| {
                control.status == RestrictedQualificationControlStatus::ReviewPending
            })
    }

    /// Reports whether all private controls are frozen by a review receipt.
    #[must_use]
    pub fn is_frozen_reviewed(&self) -> bool {
        self.review_receipt_identity.is_some()
            && self.controls.iter().all(|control| {
                control.status == RestrictedQualificationControlStatus::FrozenReviewed
            })
    }

    /// Returns the redacted independent-review receipt identity.
    #[must_use]
    pub const fn review_receipt_identity(&self) -> Option<QualificationControlReviewReceiptId> {
        self.review_receipt_identity
    }
}

impl TryFrom<IntentRestrictedQualificationSummaryWire> for IntentRestrictedQualificationSummaryV1 {
    type Error = QualificationFixtureError;

    fn try_from(wire: IntentRestrictedQualificationSummaryWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            review_receipt_identity: wire.review_receipt_identity,
            controls: wire.controls,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for IntentRestrictedQualificationSummaryV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentRestrictedQualificationSummaryWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Independent checks required to freeze private qualification controls.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualificationControlReviewCheck {
    GoldenIndependence,
    WrongBindingValidity,
    RedactionCanaryValidity,
    ExposureAndDiagnosticSafety,
}

impl QualificationControlReviewCheck {
    fn all() -> [Self; 4] {
        [
            Self::GoldenIndependence,
            Self::WrongBindingValidity,
            Self::RedactionCanaryValidity,
            Self::ExposureAndDiagnosticSafety,
        ]
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum QualificationControlReviewOutcome {
    Accepted,
}

/// Private receipt proving non-author review of exact public and private qualification controls.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentQualificationControlReviewReceiptV1 {
    schema_version: SchemaV1,
    review_subject_identity: IntentQualificationReviewSubjectIdentity,
    control_manifest_identity: RestrictedQualificationManifestId,
    control_author: QualificationControlAuthorId,
    reviewer: QualificationControlReviewerId,
    checks: Vec<QualificationControlReviewCheck>,
    controls: Vec<RestrictedQualificationControlKind>,
    outcome: QualificationControlReviewOutcome,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentQualificationControlReviewReceiptWire {
    schema_version: SchemaV1,
    review_subject_identity: IntentQualificationReviewSubjectIdentity,
    control_manifest_identity: RestrictedQualificationManifestId,
    control_author: QualificationControlAuthorId,
    reviewer: QualificationControlReviewerId,
    checks: Vec<QualificationControlReviewCheck>,
    controls: Vec<RestrictedQualificationControlKind>,
    outcome: QualificationControlReviewOutcome,
}

impl IntentQualificationControlReviewReceiptV1 {
    fn validate(&self) -> Result<(), QualificationFixtureError> {
        if self.control_author.as_str() != "qualification-control-author-ws-quality"
            || self.checks != QualificationControlReviewCheck::all()
            || self.controls != RestrictedQualificationControlKind::all()
        {
            return Err(QualificationFixtureError::InconsistentFixture);
        }
        Ok(())
    }

    /// Derives the redacted public receipt identity from exact canonical bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when the content identity frame is invalid.
    pub fn identity(
        bytes: &[u8],
    ) -> Result<QualificationControlReviewReceiptId, QualificationFixtureError> {
        QualificationControlReviewReceiptId::derive(bytes)
    }

    /// Returns the exact reviewed private manifest identity.
    #[must_use]
    pub const fn control_manifest_identity(&self) -> RestrictedQualificationManifestId {
        self.control_manifest_identity
    }

    /// Returns the exact reviewed public subject identity.
    #[must_use]
    pub const fn review_subject_identity(&self) -> IntentQualificationReviewSubjectIdentity {
        self.review_subject_identity
    }

    /// Returns the non-author reviewer identity.
    #[must_use]
    pub const fn reviewer(&self) -> &QualificationControlReviewerId {
        &self.reviewer
    }
}

impl TryFrom<IntentQualificationControlReviewReceiptWire>
    for IntentQualificationControlReviewReceiptV1
{
    type Error = QualificationFixtureError;

    fn try_from(wire: IntentQualificationControlReviewReceiptWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            review_subject_identity: wire.review_subject_identity,
            control_manifest_identity: wire.control_manifest_identity,
            control_author: wire.control_author,
            reviewer: wire.reviewer,
            checks: wire.checks,
            controls: wire.controls,
            outcome: wire.outcome,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for IntentQualificationControlReviewReceiptV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentQualificationControlReviewReceiptWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
enum QualificationArtifactRole {
    Documentation,
    StrictV1IdentityControls,
    AbiStaticFactsControls,
    RequiredIntentEvidenceControls,
    RecipePlanValidationControls,
    RecordedHostRunnerControls,
    RawObservationComparisonControls,
    ReceiptClosureControls,
    IntentPolicyControls,
    IntentGateControls,
    DiagnosticRedactionControls,
    MechanismContracts,
    RequalificationPlans,
    RestrictedControlSummary,
    ReviewAssignments,
}

impl QualificationArtifactRole {
    fn expected_path(self) -> &'static str {
        match self {
            Self::Documentation => concat!(
                "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/",
                "README.md"
            ),
            Self::StrictV1IdentityControls => concat!(
                "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/",
                "controls/01-strict-v1-identity.json"
            ),
            Self::AbiStaticFactsControls => concat!(
                "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/",
                "controls/02-abi-static-facts.json"
            ),
            Self::RequiredIntentEvidenceControls => concat!(
                "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/",
                "controls/03-required-evidence.json"
            ),
            Self::RecipePlanValidationControls => concat!(
                "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/",
                "controls/04-recipe-plan-validation.json"
            ),
            Self::RecordedHostRunnerControls => concat!(
                "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/",
                "controls/05-recorded-host-runner.json"
            ),
            Self::RawObservationComparisonControls => concat!(
                "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/",
                "controls/06-raw-observation-comparison.json"
            ),
            Self::ReceiptClosureControls => concat!(
                "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/",
                "controls/07-receipt-closure.json"
            ),
            Self::IntentPolicyControls => concat!(
                "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/",
                "controls/08-intent-policy.json"
            ),
            Self::IntentGateControls => concat!(
                "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/",
                "controls/09-intent-gate.json"
            ),
            Self::DiagnosticRedactionControls => concat!(
                "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/",
                "controls/10-diagnostic-redaction.json"
            ),
            Self::MechanismContracts => concat!(
                "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/",
                "mechanism-contracts.json"
            ),
            Self::RequalificationPlans => concat!(
                "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/",
                "requalification-plans.json"
            ),
            Self::RestrictedControlSummary => concat!(
                "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/",
                "restricted-controls.public.json"
            ),
            Self::ReviewAssignments => concat!(
                "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/",
                "review-assignments.json"
            ),
        }
    }

    fn control_slot(self) -> Option<QualificationMechanismSlot> {
        match self {
            Self::StrictV1IdentityControls => Some(QualificationMechanismSlot::StrictV1Identity),
            Self::AbiStaticFactsControls => Some(QualificationMechanismSlot::AbiStaticFacts),
            Self::RequiredIntentEvidenceControls => {
                Some(QualificationMechanismSlot::RequiredIntentEvidence)
            }
            Self::RecipePlanValidationControls => {
                Some(QualificationMechanismSlot::RecipePlanValidation)
            }
            Self::RecordedHostRunnerControls => {
                Some(QualificationMechanismSlot::RecordedHostRunner)
            }
            Self::RawObservationComparisonControls => {
                Some(QualificationMechanismSlot::RawObservationComparison)
            }
            Self::ReceiptClosureControls => Some(QualificationMechanismSlot::ReceiptClosure),
            Self::IntentPolicyControls => Some(QualificationMechanismSlot::IntentPolicy),
            Self::IntentGateControls => Some(QualificationMechanismSlot::IntentGate),
            Self::DiagnosticRedactionControls => {
                Some(QualificationMechanismSlot::DiagnosticRedaction)
            }
            Self::Documentation
            | Self::MechanismContracts
            | Self::RequalificationPlans
            | Self::RestrictedControlSummary
            | Self::ReviewAssignments => None,
        }
    }

    fn all() -> [Self; 15] {
        [
            Self::Documentation,
            Self::StrictV1IdentityControls,
            Self::AbiStaticFactsControls,
            Self::RequiredIntentEvidenceControls,
            Self::RecipePlanValidationControls,
            Self::RecordedHostRunnerControls,
            Self::RawObservationComparisonControls,
            Self::ReceiptClosureControls,
            Self::IntentPolicyControls,
            Self::IntentGateControls,
            Self::DiagnosticRedactionControls,
            Self::MechanismContracts,
            Self::RequalificationPlans,
            Self::RestrictedControlSummary,
            Self::ReviewAssignments,
        ]
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct QualificationArtifactEntryV1 {
    path: IntentQualificationArtifactPath,
    role: QualificationArtifactRole,
    identity: IntentQualificationArtifactIdentity,
    control_identity: Option<QualificationControlIdentity>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum QualificationSpecificationDecision {
    #[serde(rename = "D-040")]
    D040,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct QualificationSpecificationReferenceV1 {
    decision: QualificationSpecificationDecision,
    commit: GitCommitId,
    path: RepositoryPath,
}

/// Public current-V1 manifest binding D-040 controls to exact DEV-001 inputs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentQualificationPublicManifestV1 {
    schema_version: SchemaV1,
    owner_slice: DevelopmentSliceId,
    author: FixtureAuthorId,
    license: FixtureLicense,
    data_classification: PublicDataClassification,
    entry_commit: GitCommitId,
    intent_bundle_identity: IntentBundleIdentity,
    intent_review_receipt_identity: RestrictedReviewReceiptId,
    specification: QualificationSpecificationReferenceV1,
    assets: Vec<QualificationArtifactEntryV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentQualificationPublicManifestWire {
    schema_version: SchemaV1,
    owner_slice: DevelopmentSliceId,
    author: FixtureAuthorId,
    license: FixtureLicense,
    data_classification: PublicDataClassification,
    entry_commit: GitCommitId,
    intent_bundle_identity: IntentBundleIdentity,
    intent_review_receipt_identity: RestrictedReviewReceiptId,
    specification: QualificationSpecificationReferenceV1,
    assets: Vec<QualificationArtifactEntryV1>,
}

impl IntentQualificationPublicManifestV1 {
    fn validate(&self) -> Result<(), QualificationFixtureError> {
        if self.owner_slice.as_str() != "dev-002"
            || self.author.as_str() != "qualification-control-author-ws-quality"
            || self.entry_commit.as_str() != "9b2502d134e899ef76da869b3bac00c962130586"
            || self.intent_bundle_identity.to_string() != DEV001_BUNDLE
            || self.intent_review_receipt_identity.to_string() != DEV001_REVIEW_RECEIPT
            || self.specification.commit.as_str() != "baab47b21ad9e82ffa3d683123812f975325c3d1"
            || self.specification.path.as_str() != "docs/DECISIONS.md"
            || self.assets.len() != QualificationArtifactRole::all().len()
        {
            return Err(QualificationFixtureError::InconsistentFixture);
        }
        let mut prior: Option<&str> = None;
        let mut roles = BTreeSet::new();
        for (asset, role) in self.assets.iter().zip(QualificationArtifactRole::all()) {
            if asset.role != role
                || asset.path.as_str() != role.expected_path()
                || prior.is_some_and(|value| value >= asset.path.as_str())
                || !roles.insert(role)
                || asset.control_identity.is_some() != role.control_slot().is_some()
            {
                return Err(QualificationFixtureError::NonCanonicalSet {
                    field: "qualification manifest assets",
                });
            }
            prior = Some(asset.path.as_str());
        }
        Ok(())
    }

    /// Recomputes public artifact and semantic control identities.
    ///
    /// # Errors
    ///
    /// Fails on missing files, malformed controls, or an identity mismatch.
    pub fn validate_tree(&self, repository_root: &Path) -> Result<(), QualificationFixtureError> {
        self.validate()?;
        for asset in &self.assets {
            let bytes = fs::read(repository_root.join(asset.path.as_str())).map_err(|error| {
                QualificationFixtureError::Io {
                    message: error.to_string(),
                }
            })?;
            if IntentQualificationArtifactIdentity::derive(&bytes)? != asset.identity {
                return Err(QualificationFixtureError::IdentityMismatch {
                    kind: "public qualification artifact",
                });
            }
            if let Some(slot) = asset.role.control_slot() {
                let suite = decode_intent_qualification_control_suite_v1(&bytes)?;
                if suite.slot != slot
                    || Some(IntentQualificationControlSuiteV1::identity(&bytes)?)
                        != asset.control_identity
                {
                    return Err(QualificationFixtureError::IdentityMismatch {
                        kind: "qualification control",
                    });
                }
            }
        }
        if !repository_root
            .join(self.specification.path.as_str())
            .is_file()
        {
            return Err(QualificationFixtureError::InvalidValue {
                kind: "qualification specification path",
            });
        }
        Ok(())
    }

    /// Returns the exact public artifact count.
    #[must_use]
    pub fn asset_count(&self) -> usize {
        self.assets.len()
    }

    /// Derives the final public bundle identity from exact manifest bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when the content identity frame is invalid.
    pub fn identity(
        bytes: &[u8],
    ) -> Result<IntentQualificationBundleIdentity, QualificationFixtureError> {
        IntentQualificationBundleIdentity::derive(bytes)
    }
}

impl TryFrom<IntentQualificationPublicManifestWire> for IntentQualificationPublicManifestV1 {
    type Error = QualificationFixtureError;

    fn try_from(wire: IntentQualificationPublicManifestWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            owner_slice: wire.owner_slice,
            author: wire.author,
            license: wire.license,
            data_classification: wire.data_classification,
            entry_commit: wire.entry_commit,
            intent_bundle_identity: wire.intent_bundle_identity,
            intent_review_receipt_identity: wire.intent_review_receipt_identity,
            specification: wire.specification,
            assets: wire.assets,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for IntentQualificationPublicManifestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentQualificationPublicManifestWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Strictly decodes the canonical current-V1 mechanism contract set.
///
/// # Errors
///
/// Rejects noncanonical, non-V1, incomplete, inconsistent, or unknown input.
pub fn decode_intent_mechanism_contracts_v1(
    bytes: &[u8],
) -> Result<IntentMechanismQualificationContractSetV1, QualificationFixtureError> {
    decode_and_validate(bytes, IntentMechanismQualificationContractSetV1::validate)
}

/// Strictly decodes the canonical current-V1 review assignments.
///
/// # Errors
///
/// Rejects incomplete, noncanonical, self-reviewed, or unknown input.
pub fn decode_intent_qualification_review_assignments_v1(
    bytes: &[u8],
) -> Result<IntentQualificationReviewAssignmentsV1, QualificationFixtureError> {
    decode_and_validate(bytes, IntentQualificationReviewAssignmentsV1::validate)
}

/// Strictly decodes the canonical current-V1 requalification plans.
///
/// # Errors
///
/// Rejects incomplete, noncanonical, inconsistent, or unknown input.
pub fn decode_intent_requalification_plans_v1(
    bytes: &[u8],
) -> Result<IntentRequalificationPlansV1, QualificationFixtureError> {
    decode_and_validate(bytes, IntentRequalificationPlansV1::validate)
}

/// Strictly decodes one canonical current-V1 qualification control suite.
///
/// # Errors
///
/// Rejects missing, reordered, inconsistent, non-V1, or unknown input.
pub fn decode_intent_qualification_control_suite_v1(
    bytes: &[u8],
) -> Result<IntentQualificationControlSuiteV1, QualificationFixtureError> {
    decode_and_validate(bytes, IntentQualificationControlSuiteV1::validate)
}

/// Strictly decodes the redacted private qualification-control summary.
///
/// # Errors
///
/// Rejects inconsistent review state, missing categories, non-V1, or unknown input.
pub fn decode_intent_restricted_qualification_summary_v1(
    bytes: &[u8],
) -> Result<IntentRestrictedQualificationSummaryV1, QualificationFixtureError> {
    decode_and_validate(bytes, IntentRestrictedQualificationSummaryV1::validate)
}

/// Strictly decodes an accepted private qualification-control review receipt.
///
/// # Errors
///
/// Rejects incomplete, noncanonical, wrong-domain, non-V1, or self-inconsistent input.
pub fn decode_intent_qualification_control_review_receipt_v1(
    bytes: &[u8],
) -> Result<IntentQualificationControlReviewReceiptV1, QualificationFixtureError> {
    decode_and_validate(bytes, IntentQualificationControlReviewReceiptV1::validate)
}

/// Strictly decodes the public qualification bundle manifest.
///
/// # Errors
///
/// Rejects incomplete, noncanonical, wrong-input, non-V1, or unknown input.
pub fn decode_intent_qualification_manifest_v1(
    bytes: &[u8],
) -> Result<IntentQualificationPublicManifestV1, QualificationFixtureError> {
    decode_and_validate(bytes, IntentQualificationPublicManifestV1::validate)
}

/// Verifies that receipt publication changed only restricted-control authority projection.
///
/// # Errors
///
/// Fails if the receipt binds other bytes, any non-summary edge changed, or review is incomplete.
pub fn validate_intent_qualification_freeze_transition(
    review_subject_manifest_bytes: &[u8],
    reviewed_control_manifest_identity: RestrictedQualificationManifestId,
    accepted_manifest: &IntentQualificationPublicManifestV1,
    accepted_summary: &IntentRestrictedQualificationSummaryV1,
    receipt_bytes: &[u8],
) -> Result<(), QualificationFixtureError> {
    let review_manifest = decode_intent_qualification_manifest_v1(review_subject_manifest_bytes)?;
    let receipt = decode_intent_qualification_control_review_receipt_v1(receipt_bytes)?;
    if IntentQualificationReviewSubjectIdentity::derive(review_subject_manifest_bytes)?
        != receipt.review_subject_identity
        || reviewed_control_manifest_identity != receipt.control_manifest_identity
        || accepted_summary.review_receipt_identity
            != Some(IntentQualificationControlReviewReceiptV1::identity(
                receipt_bytes,
            )?)
        || !accepted_summary.is_frozen_reviewed()
        || review_manifest.schema_version != accepted_manifest.schema_version
        || review_manifest.owner_slice != accepted_manifest.owner_slice
        || review_manifest.author != accepted_manifest.author
        || review_manifest.license != accepted_manifest.license
        || review_manifest.data_classification != accepted_manifest.data_classification
        || review_manifest.entry_commit != accepted_manifest.entry_commit
        || review_manifest.intent_bundle_identity != accepted_manifest.intent_bundle_identity
        || review_manifest.intent_review_receipt_identity
            != accepted_manifest.intent_review_receipt_identity
        || review_manifest.specification != accepted_manifest.specification
        || review_manifest.assets.len() != accepted_manifest.assets.len()
    {
        return Err(QualificationFixtureError::InconsistentFixture);
    }
    for (review_asset, accepted_asset) in
        review_manifest.assets.iter().zip(&accepted_manifest.assets)
    {
        if review_asset.path != accepted_asset.path
            || review_asset.role != accepted_asset.role
            || review_asset.control_identity != accepted_asset.control_identity
            || (review_asset.role != QualificationArtifactRole::RestrictedControlSummary
                && review_asset.identity != accepted_asset.identity)
        {
            return Err(QualificationFixtureError::InconsistentFixture);
        }
    }
    Ok(())
}

fn decode_and_validate<T>(
    bytes: &[u8],
    validate: impl FnOnce(&T) -> Result<(), QualificationFixtureError>,
) -> Result<T, QualificationFixtureError>
where
    T: serde::de::DeserializeOwned,
{
    let value =
        cairn_codec::from_slice(bytes).map_err(|error| QualificationFixtureError::Codec {
            message: error.to_string(),
        })?;
    validate(&value)?;
    Ok(value)
}
