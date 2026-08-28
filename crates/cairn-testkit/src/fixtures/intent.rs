use std::{collections::BTreeSet, fmt, fs, path::Path, str::FromStr};

use cairn_protocol::{ContentId, ContentType, IdentityError};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

use super::{
    DevelopmentSliceId, FixtureAuthorId, FixtureLicense, GitCommitId, PublicDataClassification,
    RepositoryPath,
};

const INTENT_ROOT: &str = "fixtures/cuda-ascend/intent/reduce-sum-f32/v1/";
const MAX_CASE_ID_LEN: usize = 96;

/// Strict current-V1 validation failure for the first Intent materialization bundle.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum IntentFixtureError {
    #[error("intent fixture schema version must be 1")]
    UnsupportedSchemaVersion,
    #[error("{kind} is not canonical")]
    InvalidValue { kind: &'static str },
    #[error("{kind} is outside the D-039 first domain")]
    OutsideFirstDomain { kind: &'static str },
    #[error("{field} must contain the exact canonical required set")]
    NonCanonicalSet { field: &'static str },
    #[error("intent fixture fields are inconsistent")]
    InconsistentFixture,
    #[error("declared {kind} identity does not match exact bytes")]
    IdentityMismatch { kind: &'static str },
    #[error("intent fixture codec error: {message}")]
    Codec { message: String },
    #[error("intent fixture identity error: {message}")]
    Identity { message: String },
    #[error("intent fixture I/O error: {message}")]
    Io { message: String },
}

impl From<IdentityError> for IntentFixtureError {
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
                IntentFixtureError::UnsupportedSchemaVersion,
            )),
        }
    }
}

fn validate_case_id(value: &str) -> Result<(), IntentFixtureError> {
    if value.is_empty()
        || value.len() > MAX_CASE_ID_LEN
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.' | b'_')
        })
    {
        return Err(IntentFixtureError::InvalidValue { kind: "case id" });
    }
    Ok(())
}

/// Stable identity of one public Intent control case.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct IntentCaseId(String);

impl IntentCaseId {
    /// Creates a canonical case identity.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversized, or non-canonical labels.
    pub fn new(value: impl Into<String>) -> Result<Self, IntentFixtureError> {
        let value = value.into();
        validate_case_id(&value)?;
        Ok(Self(value))
    }

    /// Returns the canonical wire value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for IntentCaseId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Element count, distinct from byte length, for the D-039 first domain.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ReductionElementCount(u16);

impl ReductionElementCount {
    /// Creates an element count in `1..=256`.
    ///
    /// # Errors
    ///
    /// Rejects zero and counts wider than the D-039 first domain.
    pub fn new(value: u16) -> Result<Self, IntentFixtureError> {
        if !(1..=256).contains(&value) {
            return Err(IntentFixtureError::OutsideFirstDomain {
                kind: "element count",
            });
        }
        Ok(Self(value))
    }

    /// Returns the number of binary32 elements.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ReductionElementCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u16::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Exact binary32 datum accepted by D-039: normal or signed zero, with magnitude at most 65536.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct F32Datum(u32);

impl F32Datum {
    /// Validates exact IEEE-754 binary32 bits against the first-domain constraints.
    ///
    /// # Errors
    ///
    /// Rejects subnormal, non-finite, or out-of-range values.
    pub fn from_bits(bits: u32) -> Result<Self, IntentFixtureError> {
        let magnitude = bits & 0x7fff_ffff;
        let exponent = magnitude & 0x7f80_0000;
        let fraction = magnitude & 0x007f_ffff;
        let is_zero = exponent == 0 && fraction == 0;
        let is_normal = exponent != 0 && exponent != 0x7f80_0000;
        if !(is_zero || is_normal) || f32::from_bits(magnitude) > 65_536.0 {
            return Err(IntentFixtureError::OutsideFirstDomain {
                kind: "binary32 datum",
            });
        }
        Ok(Self(bits))
    }

    /// Returns exact IEEE-754 bits.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }
}

impl fmt::Debug for F32Datum {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "F32Datum(0x{:08x})", self.0)
    }
}

impl Serialize for F32Datum {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("0x{:08x}", self.0))
    }
}

impl<'de> Deserialize<'de> for F32Datum {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.len() != 10 || !value.starts_with("0x") {
            return Err(de::Error::custom(IntentFixtureError::InvalidValue {
                kind: "binary32 wire bits",
            }));
        }
        let bits = u32::from_str_radix(&value[2..], 16).map_err(|_| {
            de::Error::custom(IntentFixtureError::InvalidValue {
                kind: "binary32 wire bits",
            })
        })?;
        if format!("0x{bits:08x}") != value {
            return Err(de::Error::custom(IntentFixtureError::InvalidValue {
                kind: "binary32 wire bits",
            }));
        }
        Self::from_bits(bits).map_err(de::Error::custom)
    }
}

/// Public path owned by the D-039 materialization bundle.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct IntentArtifactPath(String);

impl IntentArtifactPath {
    /// Validates one exact repository-relative public bundle path.
    ///
    /// # Errors
    ///
    /// Rejects absolute, traversing, private, or out-of-bundle paths.
    pub fn new(value: impl Into<String>) -> Result<Self, IntentFixtureError> {
        let value = value.into();
        let path = Path::new(&value);
        if !value.starts_with(INTENT_ROOT)
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
            return Err(IntentFixtureError::InvalidValue {
                kind: "public Intent artifact path",
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

impl<'de> Deserialize<'de> for IntentArtifactPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

pub enum IntentPublicArtifact {}
impl ContentType for IntentPublicArtifact {
    const DOMAIN: &'static str = "testkit.intent-public-artifact.v1";
}

pub enum IntentPublicBundle {}
impl ContentType for IntentPublicBundle {
    const DOMAIN: &'static str = "testkit.intent-public-bundle.v1";
}

pub enum RestrictedReviewReceipt {}
impl ContentType for RestrictedReviewReceipt {
    const DOMAIN: &'static str = "testkit.restricted-review-receipt.v1";
}

pub enum RestrictedIntentCase {}
impl ContentType for RestrictedIntentCase {
    const DOMAIN: &'static str = "testkit.restricted-intent-case.v1";
}

pub enum RestrictedIntentManifest {}
impl ContentType for RestrictedIntentManifest {
    const DOMAIN: &'static str = "testkit.restricted-intent-manifest.v1";
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
            pub fn derive(bytes: &[u8]) -> Result<Self, IntentFixtureError> {
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
    /// Exact identity of one public source or JSON artifact.
    ///
    /// ```compile_fail
    /// use cairn_testkit::fixtures::{IntentArtifactIdentity, IntentBundleIdentity};
    /// fn require_artifact(_: IntentArtifactIdentity) {}
    /// let bundle: IntentBundleIdentity = todo!();
    /// require_artifact(bundle);
    /// ```
    IntentArtifactIdentity,
    IntentPublicArtifact,
    "Intent artifact identity"
);
typed_identity!(
    /// Exact identity of the public manifest bytes as one bundle.
    IntentBundleIdentity,
    IntentPublicBundle,
    "Intent bundle identity"
);
typed_identity!(
    /// Redacted identity of a private-store review receipt, never a case identity.
    RestrictedReviewReceiptId,
    RestrictedReviewReceipt,
    "restricted review receipt identity"
);
typed_identity!(
    /// Exact private case identity; this type must never enter a public fixture manifest.
    RestrictedIntentCaseId,
    RestrictedIntentCase,
    "restricted Intent case identity"
);
typed_identity!(
    /// Exact identity of the private sealed-batch manifest.
    RestrictedIntentManifestId,
    RestrictedIntentManifest,
    "restricted Intent manifest identity"
);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IntentArtifactRole {
    Claims,
    CmakeBuild,
    Documentation,
    HostLaunch,
    KernelSource,
    PublicCorpus,
    ReferenceObservationWriter,
    RestrictedPartitionSummary,
    UserDecisionControls,
    AbiHeader,
}

impl IntentArtifactRole {
    fn expected_path(self) -> &'static str {
        match self {
            Self::Claims => "fixtures/cuda-ascend/intent/reduce-sum-f32/v1/claims.json",
            Self::CmakeBuild => {
                "fixtures/cuda-ascend/intent/reduce-sum-f32/v1/source/CMakeLists.txt"
            }
            Self::Documentation => "fixtures/cuda-ascend/intent/reduce-sum-f32/v1/README.md",
            Self::HostLaunch => {
                "fixtures/cuda-ascend/intent/reduce-sum-f32/v1/source/src/reduce_sum_launch.cu"
            }
            Self::KernelSource => {
                "fixtures/cuda-ascend/intent/reduce-sum-f32/v1/source/src/reduce_sum_kernel.cu"
            }
            Self::PublicCorpus => {
                "fixtures/cuda-ascend/intent/reduce-sum-f32/v1/public-corpus.json"
            }
            Self::ReferenceObservationWriter => {
                "fixtures/cuda-ascend/intent/reduce-sum-f32/v1/source/tests/reference_main.cpp"
            }
            Self::RestrictedPartitionSummary => {
                "fixtures/cuda-ascend/intent/reduce-sum-f32/v1/restricted-partitions.public.json"
            }
            Self::UserDecisionControls => {
                "fixtures/cuda-ascend/intent/reduce-sum-f32/v1/user-decision-controls.json"
            }
            Self::AbiHeader => {
                "fixtures/cuda-ascend/intent/reduce-sum-f32/v1/source/include/reduce_sum.h"
            }
        }
    }

    fn all() -> [Self; 10] {
        [
            Self::Claims,
            Self::CmakeBuild,
            Self::Documentation,
            Self::AbiHeader,
            Self::HostLaunch,
            Self::KernelSource,
            Self::PublicCorpus,
            Self::ReferenceObservationWriter,
            Self::RestrictedPartitionSummary,
            Self::UserDecisionControls,
        ]
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct IntentArtifactEntryV1 {
    path: IntentArtifactPath,
    role: IntentArtifactRole,
    identity: IntentArtifactIdentity,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum IntentSpecificationDecision {
    #[serde(rename = "D-039")]
    D039,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct IntentSpecificationReferenceV1 {
    decision: IntentSpecificationDecision,
    commit: GitCommitId,
    path: RepositoryPath,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentPublicManifestV1 {
    schema_version: SchemaV1,
    owner_slice: DevelopmentSliceId,
    author: FixtureAuthorId,
    license: FixtureLicense,
    data_classification: PublicDataClassification,
    specification: IntentSpecificationReferenceV1,
    assets: Vec<IntentArtifactEntryV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentPublicManifestWire {
    schema_version: SchemaV1,
    owner_slice: DevelopmentSliceId,
    author: FixtureAuthorId,
    license: FixtureLicense,
    data_classification: PublicDataClassification,
    specification: IntentSpecificationReferenceV1,
    assets: Vec<IntentArtifactEntryV1>,
}

impl TryFrom<IntentPublicManifestWire> for IntentPublicManifestV1 {
    type Error = IntentFixtureError;

    fn try_from(wire: IntentPublicManifestWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            owner_slice: wire.owner_slice,
            author: wire.author,
            license: wire.license,
            data_classification: wire.data_classification,
            specification: wire.specification,
            assets: wire.assets,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for IntentPublicManifestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentPublicManifestWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl IntentPublicManifestV1 {
    fn validate(&self) -> Result<(), IntentFixtureError> {
        if self.owner_slice.as_str() != "dev-001"
            || self.author.as_str() != "cairn-project-ws-domain"
            || self.specification.path.as_str() != "docs/DECISIONS.md"
            || self.assets.len() != IntentArtifactRole::all().len()
        {
            return Err(IntentFixtureError::InconsistentFixture);
        }
        let mut prior: Option<&str> = None;
        let mut roles = BTreeSet::new();
        for asset in &self.assets {
            if asset.path.as_str() != asset.role.expected_path()
                || prior.is_some_and(|value| value >= asset.path.as_str())
                || !roles.insert(asset.role)
            {
                return Err(IntentFixtureError::NonCanonicalSet { field: "assets" });
            }
            prior = Some(asset.path.as_str());
        }
        if roles != IntentArtifactRole::all().into_iter().collect() {
            return Err(IntentFixtureError::NonCanonicalSet { field: "assets" });
        }
        Ok(())
    }

    /// Recomputes every artifact identity and verifies the specification reference exists.
    ///
    /// # Errors
    ///
    /// Fails on a missing artifact/specification, path error, or identity mismatch.
    pub fn validate_tree(&self, repository_root: &Path) -> Result<(), IntentFixtureError> {
        self.validate()?;
        for asset in &self.assets {
            let bytes = fs::read(repository_root.join(asset.path.as_str())).map_err(|error| {
                IntentFixtureError::Io {
                    message: error.to_string(),
                }
            })?;
            if IntentArtifactIdentity::derive(&bytes)? != asset.identity {
                return Err(IntentFixtureError::IdentityMismatch {
                    kind: "public artifact",
                });
            }
        }
        if !repository_root
            .join(self.specification.path.as_str())
            .is_file()
        {
            return Err(IntentFixtureError::InvalidValue {
                kind: "specification path",
            });
        }
        Ok(())
    }

    /// Returns the exact manifest-byte bundle identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the content identity frame is invalid.
    pub fn identity(bytes: &[u8]) -> Result<IntentBundleIdentity, IntentFixtureError> {
        IntentBundleIdentity::derive(bytes)
    }

    /// Returns public asset entries for audit tooling.
    #[must_use]
    pub fn asset_count(&self) -> usize {
        self.assets.len()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IntentHypothesisKind {
    MathematicalRealSum,
    SourceTreeBitIdentity,
    DeploymentSpecialization,
    AccumulationOrderUnknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IntentHypothesisStatus {
    RequiredIntent,
    CompetingRefutedControl,
    ExplicitUnknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct IntentHypothesisV1 {
    kind: IntentHypothesisKind,
    status: IntentHypothesisStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentClaimsV1 {
    schema_version: SchemaV1,
    operator: IntentOperator,
    hypotheses: Vec<IntentHypothesisV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentClaimsWire {
    schema_version: SchemaV1,
    operator: IntentOperator,
    hypotheses: Vec<IntentHypothesisV1>,
}

impl TryFrom<IntentClaimsWire> for IntentClaimsV1 {
    type Error = IntentFixtureError;

    fn try_from(wire: IntentClaimsWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            operator: wire.operator,
            hypotheses: wire.hypotheses,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for IntentClaimsV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentClaimsWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum IntentOperator {
    ReduceSumF32,
}

impl IntentClaimsV1 {
    fn validate(&self) -> Result<(), IntentFixtureError> {
        let expected = [
            (
                IntentHypothesisKind::MathematicalRealSum,
                IntentHypothesisStatus::RequiredIntent,
            ),
            (
                IntentHypothesisKind::SourceTreeBitIdentity,
                IntentHypothesisStatus::CompetingRefutedControl,
            ),
            (
                IntentHypothesisKind::DeploymentSpecialization,
                IntentHypothesisStatus::CompetingRefutedControl,
            ),
            (
                IntentHypothesisKind::AccumulationOrderUnknown,
                IntentHypothesisStatus::ExplicitUnknown,
            ),
        ];
        if self.hypotheses.len() != expected.len()
            || !self
                .hypotheses
                .iter()
                .zip(expected)
                .all(|(actual, expected)| (actual.kind, actual.status) == expected)
        {
            return Err(IntentFixtureError::NonCanonicalSet {
                field: "hypotheses",
            });
        }
        Ok(())
    }

    /// Returns the exact number of required/competing/unknown hypotheses.
    #[must_use]
    pub fn hypothesis_count(&self) -> usize {
        self.hypotheses.len()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PublicCorpusCaseKind {
    Honest,
    TailNonPowerOfTwo,
    OrderSensitiveCancellation,
    WrongExactBit,
    WrongDeploymentSpecialization,
    ExplicitUnknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CorpusExpectation {
    SupportsMathematicalRealSum,
    RejectsCompetingHypothesis,
    UnknownOutsideFirstDomain,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PublicCorpusCaseV1 {
    case_id: IntentCaseId,
    kind: PublicCorpusCaseKind,
    element_count: Option<ReductionElementCount>,
    input_bits: Vec<F32Datum>,
    targeted_hypothesis: Option<IntentHypothesisKind>,
    expected: CorpusExpectation,
}

impl PublicCorpusCaseV1 {
    fn validate(&self) -> Result<(), IntentFixtureError> {
        let within_domain = self.element_count.is_some_and(|count| {
            usize::from(count.get()) == self.input_bits.len() && !self.input_bits.is_empty()
        });
        let consistent = match self.kind {
            PublicCorpusCaseKind::Honest
            | PublicCorpusCaseKind::TailNonPowerOfTwo
            | PublicCorpusCaseKind::OrderSensitiveCancellation => {
                within_domain
                    && self.targeted_hypothesis.is_none()
                    && self.expected == CorpusExpectation::SupportsMathematicalRealSum
            }
            PublicCorpusCaseKind::WrongExactBit => {
                within_domain
                    && self.targeted_hypothesis == Some(IntentHypothesisKind::SourceTreeBitIdentity)
                    && self.expected == CorpusExpectation::RejectsCompetingHypothesis
            }
            PublicCorpusCaseKind::WrongDeploymentSpecialization => {
                within_domain
                    && self.targeted_hypothesis
                        == Some(IntentHypothesisKind::DeploymentSpecialization)
                    && self.expected == CorpusExpectation::RejectsCompetingHypothesis
            }
            PublicCorpusCaseKind::ExplicitUnknown => {
                self.element_count.is_none()
                    && self.input_bits.is_empty()
                    && self.targeted_hypothesis
                        == Some(IntentHypothesisKind::AccumulationOrderUnknown)
                    && self.expected == CorpusExpectation::UnknownOutsideFirstDomain
            }
        };
        if !consistent {
            return Err(IntentFixtureError::InconsistentFixture);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentPublicCorpusV1 {
    schema_version: SchemaV1,
    cases: Vec<PublicCorpusCaseV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentPublicCorpusWire {
    schema_version: SchemaV1,
    cases: Vec<PublicCorpusCaseV1>,
}

impl TryFrom<IntentPublicCorpusWire> for IntentPublicCorpusV1 {
    type Error = IntentFixtureError;

    fn try_from(wire: IntentPublicCorpusWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            cases: wire.cases,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for IntentPublicCorpusV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentPublicCorpusWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl IntentPublicCorpusV1 {
    fn validate(&self) -> Result<(), IntentFixtureError> {
        let required = [
            PublicCorpusCaseKind::Honest,
            PublicCorpusCaseKind::TailNonPowerOfTwo,
            PublicCorpusCaseKind::OrderSensitiveCancellation,
            PublicCorpusCaseKind::WrongExactBit,
            PublicCorpusCaseKind::WrongDeploymentSpecialization,
            PublicCorpusCaseKind::ExplicitUnknown,
        ];
        if self.cases.len() != required.len() {
            return Err(IntentFixtureError::NonCanonicalSet {
                field: "public corpus cases",
            });
        }
        let mut ids = BTreeSet::new();
        for (case, kind) in self.cases.iter().zip(required) {
            case.validate()?;
            if case.kind != kind || !ids.insert(case.case_id.clone()) {
                return Err(IntentFixtureError::NonCanonicalSet {
                    field: "public corpus cases",
                });
            }
        }
        Ok(())
    }

    /// Returns the exact number of public control classes.
    #[must_use]
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionControlKind {
    EmptyInput,
    ExactSourceOrderRequirement,
    DefinedSourceAnomaly,
    CallerSourceSemanticConflict,
    DomainExpansion,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionControlOutcome {
    NeedsUserDecision,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DecisionControlV1 {
    case_id: IntentCaseId,
    kind: DecisionControlKind,
    expected: DecisionControlOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentUserDecisionControlsV1 {
    schema_version: SchemaV1,
    controls: Vec<DecisionControlV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentUserDecisionControlsWire {
    schema_version: SchemaV1,
    controls: Vec<DecisionControlV1>,
}

impl TryFrom<IntentUserDecisionControlsWire> for IntentUserDecisionControlsV1 {
    type Error = IntentFixtureError;

    fn try_from(wire: IntentUserDecisionControlsWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            controls: wire.controls,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for IntentUserDecisionControlsV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentUserDecisionControlsWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl IntentUserDecisionControlsV1 {
    fn validate(&self) -> Result<(), IntentFixtureError> {
        let required = [
            DecisionControlKind::EmptyInput,
            DecisionControlKind::ExactSourceOrderRequirement,
            DecisionControlKind::DefinedSourceAnomaly,
            DecisionControlKind::CallerSourceSemanticConflict,
            DecisionControlKind::DomainExpansion,
        ];
        let mut ids = BTreeSet::new();
        if self.controls.len() != required.len()
            || !self.controls.iter().zip(required).all(|(control, kind)| {
                control.kind == kind
                    && control.expected == DecisionControlOutcome::NeedsUserDecision
                    && ids.insert(control.case_id.clone())
            })
        {
            return Err(IntentFixtureError::NonCanonicalSet {
                field: "user decision controls",
            });
        }
        Ok(())
    }

    /// Returns the exact number of user-decision controls.
    #[must_use]
    pub fn control_count(&self) -> usize {
        self.controls.len()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestrictedPartitionKind {
    ImplementationArtifact,
    SourceDefect,
    DeploymentQuirk,
    CompetingPlausibleMeaning,
    GenuineUnknown,
    TamperWrongBinding,
}

impl RestrictedPartitionKind {
    fn all() -> [Self; 6] {
        [
            Self::ImplementationArtifact,
            Self::SourceDefect,
            Self::DeploymentQuirk,
            Self::CompetingPlausibleMeaning,
            Self::GenuineUnknown,
            Self::TamperWrongBinding,
        ]
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestrictedPartitionStatus {
    ReviewPending,
    FrozenReviewed,
}

/// Identity of a private-store reviewer, distinct from fixture authors and artifact identities.
///
/// ```compile_fail
/// use cairn_testkit::fixtures::{FixtureAuthorId, PrivateCorpusReviewerId};
/// fn require_reviewer(_: PrivateCorpusReviewerId) {}
/// let author: FixtureAuthorId = todo!();
/// require_reviewer(author);
/// ```
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PrivateCorpusReviewerId(String);

impl PrivateCorpusReviewerId {
    /// Creates a canonical reviewer identity under the private-reviewer namespace.
    ///
    /// # Errors
    ///
    /// Rejects identities outside `private-reviewer-*` or with noncanonical label bytes.
    pub fn new(value: impl Into<String>) -> Result<Self, IntentFixtureError> {
        let value = value.into();
        validate_case_id(&value)?;
        if !value.starts_with("private-reviewer-") {
            return Err(IntentFixtureError::InvalidValue {
                kind: "private corpus reviewer",
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

impl<'de> Deserialize<'de> for PrivateCorpusReviewerId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Independently reviewed facts required before a private case set can be frozen.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrivateReviewCheck {
    CleanRoomSourceProvenance,
    D039DomainAndAbi,
    PartitionSemanticCoverage,
    PublicDerivationIndependence,
    BindingTamperValidity,
    ExposureAndDiagnosticSafety,
}

impl PrivateReviewCheck {
    fn all() -> [Self; 6] {
        [
            Self::CleanRoomSourceProvenance,
            Self::D039DomainAndAbi,
            Self::PartitionSemanticCoverage,
            Self::PublicDerivationIndependence,
            Self::BindingTamperValidity,
            Self::ExposureAndDiagnosticSafety,
        ]
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PrivateReviewOutcome {
    Accepted,
}

/// Private independent-review receipt. It binds authority to exact public and private inputs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentPrivateReviewReceiptV1 {
    schema_version: SchemaV1,
    decision: IntentSpecificationDecision,
    public_bundle_identity: IntentBundleIdentity,
    case_set_manifest_identity: RestrictedIntentManifestId,
    case_author: FixtureAuthorId,
    reviewer: PrivateCorpusReviewerId,
    checks: Vec<PrivateReviewCheck>,
    partitions: Vec<RestrictedPartitionKind>,
    outcome: PrivateReviewOutcome,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentPrivateReviewReceiptWire {
    schema_version: SchemaV1,
    decision: IntentSpecificationDecision,
    public_bundle_identity: IntentBundleIdentity,
    case_set_manifest_identity: RestrictedIntentManifestId,
    case_author: FixtureAuthorId,
    reviewer: PrivateCorpusReviewerId,
    checks: Vec<PrivateReviewCheck>,
    partitions: Vec<RestrictedPartitionKind>,
    outcome: PrivateReviewOutcome,
}

impl IntentPrivateReviewReceiptV1 {
    fn validate(&self) -> Result<(), IntentFixtureError> {
        if self.case_author.as_str() != "cairn-project-ws-domain"
            || self.checks != PrivateReviewCheck::all()
            || self.partitions != RestrictedPartitionKind::all()
        {
            return Err(IntentFixtureError::InconsistentFixture);
        }
        Ok(())
    }

    /// Derives the redacted public reference from exact canonical receipt bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when the receipt identity frame is invalid.
    pub fn identity(bytes: &[u8]) -> Result<RestrictedReviewReceiptId, IntentFixtureError> {
        RestrictedReviewReceiptId::derive(bytes)
    }

    /// Returns the exact reviewed private case-set manifest identity.
    #[must_use]
    pub const fn case_set_manifest_identity(&self) -> RestrictedIntentManifestId {
        self.case_set_manifest_identity
    }

    /// Returns the exact reviewed public bundle identity.
    #[must_use]
    pub const fn public_bundle_identity(&self) -> IntentBundleIdentity {
        self.public_bundle_identity
    }

    /// Returns the independent reviewer identity.
    #[must_use]
    pub const fn reviewer(&self) -> &PrivateCorpusReviewerId {
        &self.reviewer
    }
}

impl TryFrom<IntentPrivateReviewReceiptWire> for IntentPrivateReviewReceiptV1 {
    type Error = IntentFixtureError;

    fn try_from(wire: IntentPrivateReviewReceiptWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            decision: wire.decision,
            public_bundle_identity: wire.public_bundle_identity,
            case_set_manifest_identity: wire.case_set_manifest_identity,
            case_author: wire.case_author,
            reviewer: wire.reviewer,
            checks: wire.checks,
            partitions: wire.partitions,
            outcome: wire.outcome,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for IntentPrivateReviewReceiptV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentPrivateReviewReceiptWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RestrictedPartitionSummaryV1 {
    partition: RestrictedPartitionKind,
    status: RestrictedPartitionStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RestrictedBatchPolicy {
    NonAdaptiveSealedBatch,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentRestrictedSummaryV1 {
    schema_version: SchemaV1,
    policy: RestrictedBatchPolicy,
    review_receipt_identity: Option<RestrictedReviewReceiptId>,
    partitions: Vec<RestrictedPartitionSummaryV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentRestrictedSummaryWire {
    schema_version: SchemaV1,
    policy: RestrictedBatchPolicy,
    review_receipt_identity: Option<RestrictedReviewReceiptId>,
    partitions: Vec<RestrictedPartitionSummaryV1>,
}

impl TryFrom<IntentRestrictedSummaryWire> for IntentRestrictedSummaryV1 {
    type Error = IntentFixtureError;

    fn try_from(wire: IntentRestrictedSummaryWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            policy: wire.policy,
            review_receipt_identity: wire.review_receipt_identity,
            partitions: wire.partitions,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for IntentRestrictedSummaryV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentRestrictedSummaryWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl IntentRestrictedSummaryV1 {
    fn validate(&self) -> Result<(), IntentFixtureError> {
        let required = RestrictedPartitionKind::all();
        let status = self.partitions.first().map(|partition| partition.status);
        let receipt_is_consistent = matches!(
            (status, self.review_receipt_identity),
            (Some(RestrictedPartitionStatus::ReviewPending), None)
                | (Some(RestrictedPartitionStatus::FrozenReviewed), Some(_))
        );
        if self.partitions.len() != required.len()
            || !receipt_is_consistent
            || !self
                .partitions
                .iter()
                .zip(required)
                .all(|(partition, kind)| {
                    partition.partition == kind && Some(partition.status) == status
                })
        {
            return Err(IntentFixtureError::NonCanonicalSet {
                field: "restricted partition summaries",
            });
        }
        Ok(())
    }

    /// Returns the six redacted required partition statuses.
    #[must_use]
    pub fn partition_count(&self) -> usize {
        self.partitions.len()
    }

    /// Returns the private-review receipt identity without exposing private artifact identities.
    #[must_use]
    pub const fn review_receipt_identity(&self) -> Option<RestrictedReviewReceiptId> {
        self.review_receipt_identity
    }

    /// Reports whether an independent private-store review still blocks freezing.
    #[must_use]
    pub fn is_review_pending(&self) -> bool {
        self.partitions
            .iter()
            .all(|partition| partition.status == RestrictedPartitionStatus::ReviewPending)
    }
}

/// Strictly decodes the canonical current-V1 public Intent manifest.
///
/// # Errors
///
/// Rejects noncanonical, non-V1, incomplete, inconsistent, or unknown input.
pub fn decode_intent_manifest_v1(
    bytes: &[u8],
) -> Result<IntentPublicManifestV1, IntentFixtureError> {
    decode_and_validate(bytes, IntentPublicManifestV1::validate)
}

/// Strictly decodes the canonical current-V1 hypothesis set.
///
/// # Errors
///
/// Rejects noncanonical, non-V1, incomplete, inconsistent, or unknown input.
pub fn decode_intent_claims_v1(bytes: &[u8]) -> Result<IntentClaimsV1, IntentFixtureError> {
    decode_and_validate(bytes, IntentClaimsV1::validate)
}

/// Strictly decodes the canonical current-V1 public corpus.
///
/// # Errors
///
/// Rejects noncanonical, non-V1, out-of-domain, incomplete, or unknown input.
pub fn decode_intent_public_corpus_v1(
    bytes: &[u8],
) -> Result<IntentPublicCorpusV1, IntentFixtureError> {
    decode_and_validate(bytes, IntentPublicCorpusV1::validate)
}

/// Strictly decodes the canonical current-V1 user-decision controls.
///
/// # Errors
///
/// Rejects noncanonical, non-V1, incomplete, inconsistent, or unknown input.
pub fn decode_intent_user_decisions_v1(
    bytes: &[u8],
) -> Result<IntentUserDecisionControlsV1, IntentFixtureError> {
    decode_and_validate(bytes, IntentUserDecisionControlsV1::validate)
}

/// Strictly decodes the redacted current-V1 restricted partition summary.
///
/// # Errors
///
/// Rejects noncanonical, non-V1, incomplete, inconsistent, or unknown input.
pub fn decode_intent_restricted_summary_v1(
    bytes: &[u8],
) -> Result<IntentRestrictedSummaryV1, IntentFixtureError> {
    decode_and_validate(bytes, IntentRestrictedSummaryV1::validate)
}

/// Strictly decodes an accepted private independent-review receipt.
///
/// # Errors
///
/// Rejects noncanonical, non-V1, incomplete, wrong-domain, or self-inconsistent input.
pub fn decode_intent_private_review_receipt_v1(
    bytes: &[u8],
) -> Result<IntentPrivateReviewReceiptV1, IntentFixtureError> {
    decode_and_validate(bytes, IntentPrivateReviewReceiptV1::validate)
}

fn decode_and_validate<T>(
    bytes: &[u8],
    validate: impl FnOnce(&T) -> Result<(), IntentFixtureError>,
) -> Result<T, IntentFixtureError>
where
    T: serde::de::DeserializeOwned,
{
    let value = cairn_codec::from_slice(bytes).map_err(|error| codec_error(&error))?;
    validate(&value)?;
    Ok(value)
}

fn codec_error(error: &cairn_codec::CodecError) -> IntentFixtureError {
    IntentFixtureError::Codec {
        message: error.to_string(),
    }
}
