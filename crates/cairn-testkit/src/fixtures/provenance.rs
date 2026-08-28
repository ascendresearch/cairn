use std::{collections::BTreeSet, fmt, path::Path, str::FromStr};

use cairn_protocol::{ContentId, ContentType, IdentityError};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

const MAX_LABEL_LEN: usize = 128;

/// Failure to decode or validate a current-V1 public fixture contract.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum FixtureError {
    /// Only the single current pre-release V1 contract is accepted.
    #[error("fixture schema version must be 1")]
    UnsupportedSchemaVersion,
    /// A semantic label was empty, oversized, or non-canonical.
    #[error("{kind} is not a valid canonical fixture label")]
    InvalidLabel { kind: &'static str },
    /// A repository path was absolute, private, non-canonical, or escaped its allowed root.
    #[error("{kind} is not an allowed repository-relative path")]
    InvalidPath { kind: &'static str },
    /// A Git commit was not one exact lowercase full object name.
    #[error("historical Git commit must be exactly 40 lowercase hexadecimal characters")]
    InvalidGitCommit,
    /// A required collection was empty, duplicated, or not in strict canonical order.
    #[error("{field} must be non-empty and in strict canonical order")]
    NonCanonicalSet { field: &'static str },
    /// A fixture family, case, obligation, or replacement scope contradicted another field.
    #[error("fixture family, case, obligation, or replacement scope is inconsistent")]
    InconsistentFixture,
    /// A declared typed identity did not match the canonical bytes it names.
    #[error("declared {kind} identity does not match canonical bytes")]
    IdentityMismatch { kind: &'static str },
    /// A fixture cited a source reference absent from the same manifest.
    #[error("fixture cites an unknown historical source reference")]
    MissingSourceReference,
    /// Canonical encoding or strict decoding failed.
    #[error("fixture codec error: {message}")]
    Codec { message: String },
    /// Typed content identity derivation failed.
    #[error("fixture identity error: {message}")]
    Identity { message: String },
}

impl From<IdentityError> for FixtureError {
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
            _ => Err(de::Error::custom(FixtureError::UnsupportedSchemaVersion)),
        }
    }
}

fn validate_label(value: &str, kind: &'static str) -> Result<(), FixtureError> {
    if value.is_empty()
        || value.len() > MAX_LABEL_LEN
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'.' | b'_' | b'/')
        })
    {
        return Err(FixtureError::InvalidLabel { kind });
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
            /// Creates a validated canonical label.
            ///
            /// # Errors
            ///
            /// Rejects empty, oversized, or non-canonical labels.
            pub fn new(value: impl Into<String>) -> Result<Self, FixtureError> {
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
    /// Exact current author role for a newly authored fixture.
    FixtureAuthorId,
    "fixture author"
);
validated_label!(
    /// Stable identity of one case inside a fixture family.
    FixtureCaseId,
    "fixture case"
);

/// Development slice that consumes or owns a fixture obligation.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct DevelopmentSliceId(String);

impl DevelopmentSliceId {
    /// Validates the canonical lower-wire form `dev-NNN`.
    ///
    /// # Errors
    ///
    /// Rejects values outside the stable development-slice namespace.
    pub fn new(value: impl Into<String>) -> Result<Self, FixtureError> {
        let value = value.into();
        if value.len() != 7
            || !value.starts_with("dev-")
            || !value[4..].bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(FixtureError::InvalidLabel {
                kind: "development slice",
            });
        }
        Ok(Self(value))
    }

    /// Returns the canonical development-slice label.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for DevelopmentSliceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// One exact full Git commit object name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct GitCommitId(String);

impl GitCommitId {
    /// Validates one full lowercase hexadecimal commit identity.
    ///
    /// # Errors
    ///
    /// Rejects abbreviated, uppercase, or non-hexadecimal values.
    pub fn new(value: impl Into<String>) -> Result<Self, FixtureError> {
        let value = value.into();
        if value.len() != 40
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(FixtureError::InvalidGitCommit);
        }
        Ok(Self(value))
    }

    /// Returns the full canonical object name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for GitCommitId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

fn validate_relative_path(value: &str, kind: &'static str) -> Result<(), FixtureError> {
    let path = Path::new(value);
    if value.is_empty()
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
    {
        return Err(FixtureError::InvalidPath { kind });
    }
    Ok(())
}

/// Repository-relative path to one current public fixture.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PublicFixturePath(String);

impl PublicFixturePath {
    /// Validates a path under the only current public regression fixture root.
    ///
    /// # Errors
    ///
    /// Rejects absolute paths, traversal, private/restricted components, and paths outside the
    /// current public fixture tree.
    pub fn new(value: impl Into<String>) -> Result<Self, FixtureError> {
        let value = value.into();
        validate_relative_path(&value, "public fixture path")?;
        let forbidden = value
            .split('/')
            .any(|part| matches!(part, ".cairn" | "secrets" | "restricted"));
        if !value.starts_with("fixtures/regressions/v1/") || forbidden {
            return Err(FixtureError::InvalidPath {
                kind: "public fixture path",
            });
        }
        Ok(Self(value))
    }

    /// Returns the repository-relative wire path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PublicFixturePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Repository-relative source path retained only as historical provenance.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RepositoryPath(String);

impl RepositoryPath {
    /// Validates a public tracked repository path.
    ///
    /// # Errors
    ///
    /// Rejects absolute/traversing paths and private path components.
    pub fn new(value: impl Into<String>) -> Result<Self, FixtureError> {
        let value = value.into();
        validate_relative_path(&value, "historical source path")?;
        if value
            .split('/')
            .any(|part| matches!(part, ".cairn" | "secrets"))
        {
            return Err(FixtureError::InvalidPath {
                kind: "historical source path",
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

impl<'de> Deserialize<'de> for RepositoryPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Semantic historical behavior retained by a newly authored fixture.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HistoricalBehavior {
    HistoricalFalseReject,
    ModelInputCompleteness,
    RecordedLiveDivergence,
    RecoverableWrongCitation,
    OutputCaptureFailure,
    StaleLease,
    DuplicateLiveWorker,
}

/// Immutable source reference; it contains no historical/private source bytes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoricalSourceReferenceV1 {
    commit: GitCommitId,
    path: RepositoryPath,
    behavior: HistoricalBehavior,
}

impl HistoricalSourceReferenceV1 {
    /// Creates a typed historical source reference.
    #[must_use]
    pub const fn new(
        commit: GitCommitId,
        path: RepositoryPath,
        behavior: HistoricalBehavior,
    ) -> Self {
        Self {
            commit,
            path,
            behavior,
        }
    }

    /// Derives the identity of the canonical reference bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or identity derivation fails.
    pub fn identity(&self) -> Result<HistoricalSourceReferenceId, FixtureError> {
        let bytes = cairn_codec::to_vec(self).map_err(|error| codec_error(&error))?;
        HistoricalSourceReferenceId::derive(&bytes)
    }

    /// Returns the cited historical path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the exact full historical commit identity.
    #[must_use]
    pub const fn commit(&self) -> &GitCommitId {
        &self.commit
    }
}

pub enum SanitizedFixtureArtifact {}
impl ContentType for SanitizedFixtureArtifact {
    const DOMAIN: &'static str = "testkit.sanitized-fixture.v1";
}

pub enum HistoricalSourceReferenceArtifact {}
impl ContentType for HistoricalSourceReferenceArtifact {
    const DOMAIN: &'static str = "testkit.historical-source-reference.v1";
}

macro_rules! content_identity {
    ($(#[$meta:meta])* $name:ident, $artifact:ty, $kind:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Eq, Hash, PartialEq)]
        pub struct $name(ContentId<$artifact>);

        impl $name {
            /// Derives the semantic identity from exact canonical bytes.
            ///
            /// # Errors
            ///
            /// Returns an error when the content identity frame is invalid.
            pub fn derive(bytes: &[u8]) -> Result<Self, FixtureError> {
                Ok(Self(ContentId::derive(bytes)?))
            }

            /// Returns the canonical tagged identity.
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

        impl PartialOrd for $name {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Ord for $name {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.to_wire().cmp(&other.to_wire())
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

content_identity!(
    /// Typed identity for exact newly authored fixture bytes.
    ///
    /// ```compile_fail
    /// use cairn_testkit::fixtures::{FixtureIdentity, HistoricalSourceReferenceId};
    /// fn require_fixture(_: FixtureIdentity) {}
    /// let source: HistoricalSourceReferenceId = todo!();
    /// require_fixture(source);
    /// ```
    FixtureIdentity,
    SanitizedFixtureArtifact,
    "fixture identity"
);
content_identity!(
    /// Typed identity for a historical source reference body, never its cited source bytes.
    HistoricalSourceReferenceId,
    HistoricalSourceReferenceArtifact,
    "historical source reference identity"
);

/// License of newly authored public fixture material.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FixtureLicense {
    Mit,
}

/// Data classification accepted by the public fixture manifest.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PublicDataClassification {
    Public,
}

/// Whether bytes are new synthetic material or copied historical material.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FixtureOriginClass {
    NewlyAuthoredSynthetic,
}

/// Exact current obligation represented by a fixture.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FixtureObligation {
    HistoricalFalseReject,
    ModelInputCompleteness,
    RecordedLiveDivergence,
    RecoverableWrongCitation,
    OutputCaptureFailure,
    StaleLease,
    DuplicateLiveWorker,
}

impl FixtureObligation {
    const fn historical_behavior(self) -> HistoricalBehavior {
        match self {
            Self::HistoricalFalseReject => HistoricalBehavior::HistoricalFalseReject,
            Self::ModelInputCompleteness => HistoricalBehavior::ModelInputCompleteness,
            Self::RecordedLiveDivergence => HistoricalBehavior::RecordedLiveDivergence,
            Self::RecoverableWrongCitation => HistoricalBehavior::RecoverableWrongCitation,
            Self::OutputCaptureFailure => HistoricalBehavior::OutputCaptureFailure,
            Self::StaleLease => HistoricalBehavior::StaleLease,
            Self::DuplicateLiveWorker => HistoricalBehavior::DuplicateLiveWorker,
        }
    }
}

/// Scope in which the new fixture replaces historical behavior bytes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReplacementScope {
    HistoricalOracleRegression,
    ModelInputAuditRegression,
    ReplayTaxonomyRegression,
    CitationRecoveryRegression,
    ExecutionCaptureRegression,
    AssignmentLeaseRegression,
    WorkerIdentityRegression,
}

/// Stable public fixture family.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FixtureFamily {
    HistoricalFalseReject,
    ModelInputAudit,
    RecordedLiveDivergence,
    WrongCitationRecovery,
    OutputCaptureFailure,
    StaleLease,
    DuplicateLiveWorker,
}

impl FixtureFamily {
    const fn obligation(self) -> FixtureObligation {
        match self {
            Self::HistoricalFalseReject => FixtureObligation::HistoricalFalseReject,
            Self::ModelInputAudit => FixtureObligation::ModelInputCompleteness,
            Self::RecordedLiveDivergence => FixtureObligation::RecordedLiveDivergence,
            Self::WrongCitationRecovery => FixtureObligation::RecoverableWrongCitation,
            Self::OutputCaptureFailure => FixtureObligation::OutputCaptureFailure,
            Self::StaleLease => FixtureObligation::StaleLease,
            Self::DuplicateLiveWorker => FixtureObligation::DuplicateLiveWorker,
        }
    }

    const fn replacement_scope(self) -> ReplacementScope {
        match self {
            Self::HistoricalFalseReject => ReplacementScope::HistoricalOracleRegression,
            Self::ModelInputAudit => ReplacementScope::ModelInputAuditRegression,
            Self::RecordedLiveDivergence => ReplacementScope::ReplayTaxonomyRegression,
            Self::WrongCitationRecovery => ReplacementScope::CitationRecoveryRegression,
            Self::OutputCaptureFailure => ReplacementScope::ExecutionCaptureRegression,
            Self::StaleLease => ReplacementScope::AssignmentLeaseRegression,
            Self::DuplicateLiveWorker => ReplacementScope::WorkerIdentityRegression,
        }
    }
}

macro_rules! wire_enum {
    ($name:ident { $($(#[$meta:meta])* $variant:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[serde(rename_all = "kebab-case")]
        pub enum $name {
            $($(#[$meta])* $variant),+
        }
    };
}

wire_enum!(HistoricalOracleRule {
    SingleSampleExact,
    MeasuredFamilySpread,
    MutationGrid
});
wire_enum!(ReductionImplementationClass {
    SequentialFold,
    BalancedTree,
    DropLastMutation
});
wire_enum!(HistoricalOracleOutcome {
    Rejected,
    Accepted,
    BlindSpot
});
wire_enum!(ModelInputAuditCondition {
    Complete,
    MissingContent,
    IntegrityMismatch,
    StorageUnavailable
});
wire_enum!(ModelInputAuditOutcome { Complete, Blocked });
wire_enum!(ReplayMode {
    RecordedWorkflow,
    SameInputLiveCounterfactual
});
wire_enum!(ReplayExpectation {
    ExactReconstruction,
    MayDiverge
});
wire_enum!(ReplayEvidenceStatus {
    Recorded,
    NotExecuted
});
wire_enum!(CitationState {
    ValidBinding,
    WrongBinding
});
wire_enum!(RecoveryOutcome {
    Resume,
    RevisionRequired
});
wire_enum!(CaptureFault {
    MissingDeclaredOutput,
    StaleOutput,
    StdoutSubstitution
});
wire_enum!(CaptureOutcome { InvalidCapture });
wire_enum!(LeaseState {
    ExpiredBeforeExecutionStart,
    ExpiredExecutionInDoubt
});
wire_enum!(LeaseDisposition {
    Reassignable,
    ReconcileOnly
});
wire_enum!(WorkerIdentityClaim {
    DifferentLiveIncarnation,
    ExpiredIncarnationReplacement,
    SameIncarnationReconnect
});
wire_enum!(WorkerIdentityOutcome {
    Rejected,
    Allowed,
    Idempotent
});

/// One strictly typed case in a newly authored public fixture.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SanitizedCaseV1 {
    HistoricalOracle {
        case_id: FixtureCaseId,
        rule: HistoricalOracleRule,
        implementation: ReductionImplementationClass,
        expected: HistoricalOracleOutcome,
    },
    ModelInputAudit {
        case_id: FixtureCaseId,
        condition: ModelInputAuditCondition,
        expected: ModelInputAuditOutcome,
    },
    Replay {
        case_id: FixtureCaseId,
        mode: ReplayMode,
        expectation: ReplayExpectation,
        evidence_status: ReplayEvidenceStatus,
    },
    CitationRecovery {
        case_id: FixtureCaseId,
        citation: CitationState,
        expected: RecoveryOutcome,
    },
    OutputCapture {
        case_id: FixtureCaseId,
        fault: CaptureFault,
        expected: CaptureOutcome,
    },
    Lease {
        case_id: FixtureCaseId,
        state: LeaseState,
        expected: LeaseDisposition,
    },
    WorkerIdentity {
        case_id: FixtureCaseId,
        claim: WorkerIdentityClaim,
        expected: WorkerIdentityOutcome,
    },
}

impl SanitizedCaseV1 {
    fn case_id(&self) -> &FixtureCaseId {
        match self {
            Self::HistoricalOracle { case_id, .. }
            | Self::ModelInputAudit { case_id, .. }
            | Self::Replay { case_id, .. }
            | Self::CitationRecovery { case_id, .. }
            | Self::OutputCapture { case_id, .. }
            | Self::Lease { case_id, .. }
            | Self::WorkerIdentity { case_id, .. } => case_id,
        }
    }

    const fn family(&self) -> FixtureFamily {
        match self {
            Self::HistoricalOracle { .. } => FixtureFamily::HistoricalFalseReject,
            Self::ModelInputAudit { .. } => FixtureFamily::ModelInputAudit,
            Self::Replay { .. } => FixtureFamily::RecordedLiveDivergence,
            Self::CitationRecovery { .. } => FixtureFamily::WrongCitationRecovery,
            Self::OutputCapture { .. } => FixtureFamily::OutputCaptureFailure,
            Self::Lease { .. } => FixtureFamily::StaleLease,
            Self::WorkerIdentity { .. } => FixtureFamily::DuplicateLiveWorker,
        }
    }

    const fn is_consistent(&self) -> bool {
        match self {
            Self::HistoricalOracle {
                rule,
                implementation,
                expected,
                ..
            } => matches!(
                (rule, implementation, expected),
                (
                    HistoricalOracleRule::SingleSampleExact,
                    ReductionImplementationClass::SequentialFold,
                    HistoricalOracleOutcome::Rejected
                ) | (
                    HistoricalOracleRule::MeasuredFamilySpread,
                    ReductionImplementationClass::BalancedTree,
                    HistoricalOracleOutcome::Accepted
                ) | (
                    HistoricalOracleRule::MutationGrid,
                    ReductionImplementationClass::DropLastMutation,
                    HistoricalOracleOutcome::BlindSpot
                )
            ),
            Self::ModelInputAudit {
                condition,
                expected,
                ..
            } => matches!(
                (condition, expected),
                (
                    ModelInputAuditCondition::Complete,
                    ModelInputAuditOutcome::Complete
                ) | (
                    ModelInputAuditCondition::MissingContent
                        | ModelInputAuditCondition::IntegrityMismatch
                        | ModelInputAuditCondition::StorageUnavailable,
                    ModelInputAuditOutcome::Blocked
                )
            ),
            Self::Replay {
                mode,
                expectation,
                evidence_status,
                ..
            } => matches!(
                (mode, expectation, evidence_status),
                (
                    ReplayMode::RecordedWorkflow,
                    ReplayExpectation::ExactReconstruction,
                    ReplayEvidenceStatus::Recorded
                ) | (
                    ReplayMode::SameInputLiveCounterfactual,
                    ReplayExpectation::MayDiverge,
                    ReplayEvidenceStatus::NotExecuted
                )
            ),
            Self::CitationRecovery {
                citation, expected, ..
            } => matches!(
                (citation, expected),
                (CitationState::ValidBinding, RecoveryOutcome::Resume)
                    | (
                        CitationState::WrongBinding,
                        RecoveryOutcome::RevisionRequired
                    )
            ),
            Self::OutputCapture { .. } => true,
            Self::Lease {
                state, expected, ..
            } => matches!(
                (state, expected),
                (
                    LeaseState::ExpiredBeforeExecutionStart,
                    LeaseDisposition::Reassignable
                ) | (
                    LeaseState::ExpiredExecutionInDoubt,
                    LeaseDisposition::ReconcileOnly
                )
            ),
            Self::WorkerIdentity {
                claim, expected, ..
            } => matches!(
                (claim, expected),
                (
                    WorkerIdentityClaim::DifferentLiveIncarnation,
                    WorkerIdentityOutcome::Rejected
                ) | (
                    WorkerIdentityClaim::ExpiredIncarnationReplacement,
                    WorkerIdentityOutcome::Allowed
                ) | (
                    WorkerIdentityClaim::SameIncarnationReconnect,
                    WorkerIdentityOutcome::Idempotent
                )
            ),
        }
    }
}

/// Strict current-V1 fixture body.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SanitizedFixtureV1 {
    schema_version: SchemaV1,
    family: FixtureFamily,
    cases: Vec<SanitizedCaseV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SanitizedFixtureWire {
    schema_version: SchemaV1,
    family: FixtureFamily,
    cases: Vec<SanitizedCaseV1>,
}

impl TryFrom<SanitizedFixtureWire> for SanitizedFixtureV1 {
    type Error = FixtureError;

    fn try_from(wire: SanitizedFixtureWire) -> Result<Self, Self::Error> {
        if wire.cases.is_empty()
            || wire
                .cases
                .windows(2)
                .any(|pair| pair[0].case_id() >= pair[1].case_id())
            || wire
                .cases
                .iter()
                .any(|case| case.family() != wire.family || !case.is_consistent())
        {
            return Err(FixtureError::InconsistentFixture);
        }
        Ok(Self {
            schema_version: wire.schema_version,
            family: wire.family,
            cases: wire.cases,
        })
    }
}

impl<'de> Deserialize<'de> for SanitizedFixtureV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SanitizedFixtureWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl SanitizedFixtureV1 {
    /// Returns the exact fixture family.
    #[must_use]
    pub const fn family(&self) -> FixtureFamily {
        self.family
    }

    /// Returns the canonical cases.
    #[must_use]
    pub fn cases(&self) -> &[SanitizedCaseV1] {
        &self.cases
    }
}

/// Manifest-owned historical source reference and its independently derived identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestSourceReferenceV1 {
    identity: HistoricalSourceReferenceId,
    body: HistoricalSourceReferenceV1,
}

impl ManifestSourceReferenceV1 {
    /// Returns the independently identified source-reference body.
    #[must_use]
    pub const fn body(&self) -> &HistoricalSourceReferenceV1 {
        &self.body
    }
}

/// One public fixture provenance entry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureManifestEntryV1 {
    identity: FixtureIdentity,
    path: PublicFixturePath,
    family: FixtureFamily,
    author: FixtureAuthorId,
    license: FixtureLicense,
    data_classification: PublicDataClassification,
    origin: FixtureOriginClass,
    obligation: FixtureObligation,
    replacement_scope: ReplacementScope,
    source_references: Vec<HistoricalSourceReferenceId>,
    consumer_slices: Vec<DevelopmentSliceId>,
}

impl FixtureManifestEntryV1 {
    /// Returns the exact fixture identity.
    #[must_use]
    pub const fn identity(&self) -> FixtureIdentity {
        self.identity
    }

    /// Returns the repository-relative fixture path.
    #[must_use]
    pub const fn path(&self) -> &PublicFixturePath {
        &self.path
    }

    /// Returns the declared family.
    #[must_use]
    pub const fn family(&self) -> FixtureFamily {
        self.family
    }
}

/// Strict current-V1 public fixture provenance manifest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FixtureManifestV1 {
    schema_version: SchemaV1,
    source_references: Vec<ManifestSourceReferenceV1>,
    fixtures: Vec<FixtureManifestEntryV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureManifestWire {
    schema_version: SchemaV1,
    source_references: Vec<ManifestSourceReferenceV1>,
    fixtures: Vec<FixtureManifestEntryV1>,
}

impl TryFrom<FixtureManifestWire> for FixtureManifestV1 {
    type Error = FixtureError;

    fn try_from(wire: FixtureManifestWire) -> Result<Self, Self::Error> {
        if wire.source_references.is_empty()
            || wire
                .source_references
                .windows(2)
                .any(|pair| pair[0].identity >= pair[1].identity)
        {
            return Err(FixtureError::NonCanonicalSet {
                field: "historical source references",
            });
        }
        for source in &wire.source_references {
            if source.body.identity()? != source.identity {
                return Err(FixtureError::IdentityMismatch {
                    kind: "historical source reference",
                });
            }
        }
        if wire.fixtures.is_empty()
            || wire
                .fixtures
                .windows(2)
                .any(|pair| pair[0].path >= pair[1].path)
        {
            return Err(FixtureError::NonCanonicalSet {
                field: "fixture entries",
            });
        }
        let source_ids: BTreeSet<_> = wire
            .source_references
            .iter()
            .map(|source| source.identity)
            .collect();
        let mut fixture_ids = BTreeSet::new();
        for entry in &wire.fixtures {
            if !fixture_ids.insert(entry.identity)
                || entry.source_references.is_empty()
                || entry
                    .source_references
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])
                || entry.consumer_slices.is_empty()
                || entry
                    .consumer_slices
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])
                || entry.family.obligation() != entry.obligation
                || entry.family.replacement_scope() != entry.replacement_scope
            {
                return Err(FixtureError::InconsistentFixture);
            }
            if entry
                .source_references
                .iter()
                .any(|identity| !source_ids.contains(identity))
            {
                return Err(FixtureError::MissingSourceReference);
            }
            if entry.source_references.iter().any(|identity| {
                wire.source_references
                    .iter()
                    .find(|source| source.identity == *identity)
                    .is_none_or(|source| {
                        source.body.behavior != entry.obligation.historical_behavior()
                    })
            }) {
                return Err(FixtureError::InconsistentFixture);
            }
        }
        Ok(Self {
            schema_version: wire.schema_version,
            source_references: wire.source_references,
            fixtures: wire.fixtures,
        })
    }
}

impl<'de> Deserialize<'de> for FixtureManifestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        FixtureManifestWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl FixtureManifestV1 {
    /// Returns the canonical historical source-reference entries.
    #[must_use]
    pub fn source_references(&self) -> &[ManifestSourceReferenceV1] {
        &self.source_references
    }

    /// Returns the canonical fixture entries.
    #[must_use]
    pub fn fixtures(&self) -> &[FixtureManifestEntryV1] {
        &self.fixtures
    }

    /// Verifies every cited source path exists and every fixture's canonical bytes match its
    /// declared identity and family.
    ///
    /// # Errors
    ///
    /// Fails on missing files, path escape, non-canonical bytes, identity mismatch, or family
    /// mismatch.
    pub fn validate_tree(&self, repository_root: &Path) -> Result<(), FixtureError> {
        for source in &self.source_references {
            if !repository_root.join(source.body.path().as_str()).is_file() {
                return Err(FixtureError::InvalidPath {
                    kind: "missing historical source path",
                });
            }
        }
        for entry in &self.fixtures {
            let bytes =
                std::fs::read(repository_root.join(entry.path.as_str())).map_err(|error| {
                    FixtureError::Codec {
                        message: error.to_string(),
                    }
                })?;
            if FixtureIdentity::derive(&bytes)? != entry.identity {
                return Err(FixtureError::IdentityMismatch { kind: "fixture" });
            }
            if decode_fixture_v1(&bytes)?.family() != entry.family {
                return Err(FixtureError::InconsistentFixture);
            }
        }
        Ok(())
    }
}

/// Strictly decodes one canonical current-V1 fixture.
///
/// # Errors
///
/// Rejects non-canonical JSON, unknown fields, non-V1 input, invalid labels, mixed families, and
/// duplicate/out-of-order cases.
pub fn decode_fixture_v1(bytes: &[u8]) -> Result<SanitizedFixtureV1, FixtureError> {
    cairn_codec::from_slice(bytes).map_err(|error| codec_error(&error))
}

/// Strictly decodes one canonical current-V1 provenance manifest.
///
/// # Errors
///
/// Rejects non-canonical JSON, unknown fields, non-V1 input, identity mismatches, missing source
/// references, and non-canonical collections.
pub fn decode_manifest_v1(bytes: &[u8]) -> Result<FixtureManifestV1, FixtureError> {
    cairn_codec::from_slice(bytes).map_err(|error| codec_error(&error))
}

fn codec_error(error: &cairn_codec::CodecError) -> FixtureError {
    FixtureError::Codec {
        message: error.to_string(),
    }
}
