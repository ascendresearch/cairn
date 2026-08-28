use std::{fmt, path::Path, str::FromStr};

use cairn_protocol::{ContentId, ContentType, IdentityError};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use super::FixtureError;

const REGRESSION_ROOT: &str = "fixtures/regressions/v1/";
const INTENT_ROOT: &str = "fixtures/cuda-ascend/intent/reduce-sum-f32/v1/";

/// Repository-relative path scanned under one approved public fixture root.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PublicSanitationPath(String);

impl PublicSanitationPath {
    /// Validates a path under either current public fixture root.
    ///
    /// # Errors
    ///
    /// Rejects paths outside the approved roots and any private, absolute, or traversing path.
    pub fn new(value: impl Into<String>) -> Result<Self, FixtureError> {
        let value = value.into();
        let path = Path::new(&value);
        let approved_root = value.starts_with(REGRESSION_ROOT) || value.starts_with(INTENT_ROOT);
        if !approved_root
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
            return Err(FixtureError::InvalidPath {
                kind: "public sanitation path",
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

/// Required public-tree sanitation class.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SanitationCheckKind {
    PrivatePath,
    AbsoluteHostPath,
    CredentialMaterial,
    ProviderBody,
    DatabaseState,
    RestrictedCase,
    NonUtf8,
}

pub enum SanitationScanProfileArtifact {}
impl ContentType for SanitationScanProfileArtifact {
    const DOMAIN: &'static str = "testkit.sanitation-scan-profile.v1";
}

/// Typed identity for the exact sanitation scan policy bytes.
///
/// ```compile_fail
/// use cairn_testkit::fixtures::{FixtureIdentity, SanitationScanProfileId};
/// fn require_profile(_: SanitationScanProfileId) {}
/// let fixture: FixtureIdentity = todo!();
/// require_profile(fixture);
/// ```
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct SanitationScanProfileId(ContentId<SanitationScanProfileArtifact>);

impl SanitationScanProfileId {
    /// Derives the profile identity from exact canonical bytes.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid content identity frame.
    pub fn derive(bytes: &[u8]) -> Result<Self, FixtureError> {
        ContentId::derive(bytes)
            .map(Self)
            .map_err(|error| FixtureError::Identity {
                message: error.to_string(),
            })
    }

    /// Returns the canonical tagged identity.
    #[must_use]
    pub fn to_wire(self) -> String {
        self.0.to_wire()
    }
}

impl fmt::Debug for SanitationScanProfileId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SanitationScanProfileId")
            .field(&self.to_wire())
            .finish()
    }
}

impl Serialize for SanitationScanProfileId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_wire())
    }
}

impl<'de> Deserialize<'de> for SanitationScanProfileId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse::<ContentId<SanitationScanProfileArtifact>>()
            .map(Self)
            .map_err(de::Error::custom)
    }
}

impl FromStr for SanitationScanProfileId {
    type Err = IdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

/// Strict current-V1 sanitation profile. All required checks are mandatory.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SanitationScanProfileV1 {
    schema_version: SchemaV1,
    checks: Vec<SanitationCheckKind>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SanitationScanProfileWire {
    schema_version: SchemaV1,
    checks: Vec<SanitationCheckKind>,
}

impl TryFrom<SanitationScanProfileWire> for SanitationScanProfileV1 {
    type Error = FixtureError;

    fn try_from(wire: SanitationScanProfileWire) -> Result<Self, Self::Error> {
        const REQUIRED: [SanitationCheckKind; 7] = [
            SanitationCheckKind::PrivatePath,
            SanitationCheckKind::AbsoluteHostPath,
            SanitationCheckKind::CredentialMaterial,
            SanitationCheckKind::ProviderBody,
            SanitationCheckKind::DatabaseState,
            SanitationCheckKind::RestrictedCase,
            SanitationCheckKind::NonUtf8,
        ];
        if wire.checks != REQUIRED {
            return Err(FixtureError::NonCanonicalSet {
                field: "sanitation checks",
            });
        }
        Ok(Self {
            schema_version: wire.schema_version,
            checks: wire.checks,
        })
    }
}

impl<'de> Deserialize<'de> for SanitationScanProfileV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SanitationScanProfileWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl SanitationScanProfileV1 {
    /// Returns the canonical required checks.
    #[must_use]
    pub fn checks(&self) -> &[SanitationCheckKind] {
        &self.checks
    }
}

/// One sanitation finding. It intentionally contains no matching content snippet.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SanitationFinding {
    path: PublicSanitationPath,
    check: SanitationCheckKind,
}

impl SanitationFinding {
    /// Returns the finding class.
    #[must_use]
    pub const fn check(&self) -> SanitationCheckKind {
        self.check
    }
}

/// Deterministic result of scanning one public fixture tree.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SanitationScanReportV1 {
    profile: SanitationScanProfileId,
    scanned_paths: Vec<PublicSanitationPath>,
    findings: Vec<SanitationFinding>,
}

impl SanitationScanReportV1 {
    /// Returns whether no prohibited public material was found.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    /// Returns the canonical scan scope.
    #[must_use]
    pub fn scanned_paths(&self) -> &[PublicSanitationPath] {
        &self.scanned_paths
    }

    /// Returns findings without sensitive snippets.
    #[must_use]
    pub fn findings(&self) -> &[SanitationFinding] {
        &self.findings
    }
}

/// Strictly decodes a canonical current-V1 sanitation scan profile.
///
/// # Errors
///
/// Rejects non-canonical, non-V1, incomplete, duplicated, reordered, or unknown input.
pub fn decode_scan_profile_v1(bytes: &[u8]) -> Result<SanitationScanProfileV1, FixtureError> {
    cairn_codec::from_slice(bytes).map_err(|error| FixtureError::Codec {
        message: error.to_string(),
    })
}

/// Recursively scans only the supplied public fixture root.
///
/// Findings expose path and category, never matched bytes. This function does not traverse
/// symlinks and has no access to `.cairn` or any restricted-store adapter.
///
/// # Errors
///
/// Returns an error when the root is not an approved public fixture path, a file cannot be read,
/// a path cannot be represented, or the scan profile cannot be identified.
pub fn scan_public_tree(
    repository_root: &Path,
    fixture_root: &Path,
    profile_bytes: &[u8],
) -> Result<SanitationScanReportV1, FixtureError> {
    let profile = decode_scan_profile_v1(profile_bytes)?;
    let profile_id = SanitationScanProfileId::derive(profile_bytes)?;
    let approved_roots = [
        repository_root.join("fixtures/regressions/v1"),
        repository_root.join("fixtures/cuda-ascend/intent/reduce-sum-f32/v1"),
    ];
    if !approved_roots.iter().any(|root| fixture_root == root) || !fixture_root.is_dir() {
        return Err(FixtureError::InvalidPath {
            kind: "public scan root",
        });
    }
    let mut files = Vec::new();
    collect_files(fixture_root, &mut files)?;
    files.sort();
    let mut scanned_paths = Vec::new();
    let mut findings = Vec::new();
    for file in files {
        let relative =
            file.strip_prefix(repository_root)
                .map_err(|_| FixtureError::InvalidPath {
                    kind: "public scan path",
                })?;
        let path = PublicSanitationPath::new(relative.to_string_lossy().replace('\\', "/"))?;
        let bytes = std::fs::read(&file).map_err(|error| FixtureError::Codec {
            message: error.to_string(),
        })?;
        scan_one(&path, &bytes, &profile, &mut findings);
        scanned_paths.push(path);
    }
    Ok(SanitationScanReportV1 {
        profile: profile_id,
        scanned_paths,
        findings,
    })
}

fn collect_files(root: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<(), FixtureError> {
    for entry in std::fs::read_dir(root).map_err(|error| FixtureError::Codec {
        message: error.to_string(),
    })? {
        let entry = entry.map_err(|error| FixtureError::Codec {
            message: error.to_string(),
        })?;
        let file_type = entry.file_type().map_err(|error| FixtureError::Codec {
            message: error.to_string(),
        })?;
        if file_type.is_symlink() {
            return Err(FixtureError::InvalidPath {
                kind: "public fixture symlink",
            });
        }
        if file_type.is_dir() {
            collect_files(&entry.path(), files)?;
        } else if file_type.is_file() {
            files.push(entry.path());
        }
    }
    Ok(())
}

fn scan_one(
    path: &PublicSanitationPath,
    bytes: &[u8],
    profile: &SanitationScanProfileV1,
    findings: &mut Vec<SanitationFinding>,
) {
    let wire_path = path.as_str();
    for check in profile.checks() {
        let matched = match check {
            SanitationCheckKind::PrivatePath => {
                wire_path.contains("/.cairn/") || wire_path.contains("/secrets/")
            }
            SanitationCheckKind::AbsoluteHostPath => {
                utf8_contains_any(bytes, &["/home/", "/Users/", "/data/projects/", "/tmp/"])
            }
            SanitationCheckKind::CredentialMaterial => utf8_contains_any(
                bytes,
                &["-----BEGIN PRIVATE KEY-----", "api_key", "credential_file"],
            ),
            SanitationCheckKind::ProviderBody => utf8_contains_any(
                bytes,
                &["raw_request", "raw_response", "private_continuation"],
            ),
            SanitationCheckKind::DatabaseState => {
                wire_path.ends_with(".sqlite3")
                    || wire_path.ends_with(".sqlite3-wal")
                    || wire_path.ends_with(".sqlite3-shm")
            }
            SanitationCheckKind::RestrictedCase => wire_path.contains("/restricted/"),
            SanitationCheckKind::NonUtf8 => std::str::from_utf8(bytes).is_err(),
        };
        if matched {
            findings.push(SanitationFinding {
                path: path.clone(),
                check: *check,
            });
        }
    }
}

fn utf8_contains_any(bytes: &[u8], needles: &[&str]) -> bool {
    std::str::from_utf8(bytes).is_ok_and(|text| needles.iter().any(|needle| text.contains(needle)))
}
