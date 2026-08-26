//! Bounded external-test research for the Oracle Agent blue role.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::Read,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use cairn_agent::{
    CanonicalToolResult, PreparedToolOperation, ToolEffectClass, ToolGateway, ToolGatewayError,
    ToolImplementationVersion, ToolName, ToolRegistration,
};
use cairn_protocol::{ContentId, ContentType, ObservedAtUnixMillis};
use cairn_record::{ContentStore, ContentStoreError};
use cairn_verification::{CorpusCaseProvenanceArtifact, LicenseProvenanceArtifact};
use reqwest::{
    StatusCode,
    blocking::Client,
    header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const SCHEMA_V1: u16 = 1;
const SEARCH_TOOL_NAME: &str = "oracle.search_external_tests";
const SEARCH_TOOL_VERSION: &str = "github-v1";
const MAX_QUERY_BYTES: usize = 256;
const MAX_REPOSITORIES: usize = 8;
const MAX_RESULTS: u16 = 10;
const MAX_PATH_BYTES: usize = 1_024;
const MAX_SOURCE_BYTES: usize = 256 * 1_024;

/// Semantic domain for an exact external-test search request.
pub enum ExternalTestSearchRequestArtifact {}

impl ContentType for ExternalTestSearchRequestArtifact {
    const DOMAIN: &'static str = "migration.external-test-search-request.v1";
}

/// Semantic domain for an exact external-test search result.
pub enum ExternalTestSearchResultArtifact {}

impl ContentType for ExternalTestSearchResultArtifact {
    const DOMAIN: &'static str = "migration.external-test-search-result.v1";
}

/// Semantic domain for exact fetched upstream source bytes.
pub enum ExternalTestSourceBytesArtifact {}

impl ContentType for ExternalTestSourceBytesArtifact {
    const DOMAIN: &'static str = "migration.external-test-source-bytes.v1";
}

macro_rules! bounded_string {
    ($(#[$meta:meta])* $name:ident, $error:literal, $validate:expr) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a validated value.
            ///
            /// # Errors
            ///
            #[doc = concat!("Returns an error when ", $error, ".")]
            pub fn new(value: impl Into<String>) -> Result<Self, ExternalResearchContractError> {
                let value = value.into();
                let valid: bool = ($validate)(&value);
                if !valid {
                    return Err(ExternalResearchContractError::InvalidField(stringify!($name)));
                }
                Ok(Self(value))
            }

            /// Returns the validated string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
            }
        }
    };
}

bounded_string!(
    /// Bounded search terms with no provider query-scope operators.
    SearchQuery,
    "the query is empty, too large, contains controls, or can alter repository scope",
    |value: &String| {
        !value.is_empty()
            && value.trim() == value
            && value.len() <= MAX_QUERY_BYTES
            && value.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || character == ' '
                    || matches!(character, '_' | '-' | '.')
            })
    }
);

bounded_string!(
    /// Canonical `owner/repository` GitHub scope selected by trusted task policy.
    GitHubRepository,
    "the repository is not one canonical owner/name pair",
    |value: &String| {
        let mut parts = value.split('/');
        let owner = parts.next().unwrap_or_default();
        let repository = parts.next().unwrap_or_default();
        let component = |part: &str| {
            !part.is_empty()
                && part.len() <= 100
                && part.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
                })
        };
        component(owner) && component(repository) && parts.next().is_none()
    }
);

bounded_string!(
    /// Relative source path returned by an admitted repository provider.
    SourcePath,
    "the source path is empty, unsafe, or too large",
    |value: &String| {
        !value.is_empty()
            && value.len() <= MAX_PATH_BYTES
            && !value.starts_with('/')
            && !value.chars().any(char::is_control)
            && value
                .split('/')
                .all(|component| !component.is_empty() && component != "." && component != "..")
    }
);

bounded_string!(
    /// Immutable upstream Git blob identity as reported and dereferenced by GitHub.
    GitHubBlobIdentity,
    "the blob identity is not a supported hexadecimal Git object identity",
    |value: &String| {
        matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    }
);

/// Positive bounded number of fetched results requested by blue.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SearchResultLimit(u16);

impl SearchResultLimit {
    /// Creates a result limit between one and the current V1 maximum.
    ///
    /// # Errors
    ///
    /// Rejects zero and values above ten.
    pub const fn new(value: u16) -> Result<Self, ExternalResearchContractError> {
        if value == 0 || value > MAX_RESULTS {
            Err(ExternalResearchContractError::InvalidResultLimit)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the wire value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for SearchResultLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(u16::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Strict model-authored search request over operator-approved repository scopes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    try_from = "ExternalTestSearchRequestWire",
    into = "ExternalTestSearchRequestWire"
)]
pub struct ExternalTestSearchRequestV1 {
    schema_version: u16,
    query: SearchQuery,
    repositories: Vec<GitHubRepository>,
    max_results: SearchResultLimit,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ExternalTestSearchRequestWire {
    schema_version: u16,
    query: SearchQuery,
    repositories: Vec<GitHubRepository>,
    max_results: SearchResultLimit,
}

impl ExternalTestSearchRequestV1 {
    /// Creates a canonical request. Repository order is semantic and must already be sorted.
    ///
    /// # Errors
    ///
    /// Rejects empty, duplicated, unsorted, or overlarge repository sets.
    pub fn new(
        query: SearchQuery,
        repositories: Vec<GitHubRepository>,
        max_results: SearchResultLimit,
    ) -> Result<Self, ExternalResearchContractError> {
        if repositories.is_empty()
            || repositories.len() > MAX_REPOSITORIES
            || repositories.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(ExternalResearchContractError::InvalidRepositories);
        }
        Ok(Self {
            schema_version: SCHEMA_V1,
            query,
            repositories,
            max_results,
        })
    }

    /// Returns the bounded search terms.
    #[must_use]
    pub const fn query(&self) -> &SearchQuery {
        &self.query
    }

    /// Returns canonical operator-selected repository scopes.
    #[must_use]
    pub fn repositories(&self) -> &[GitHubRepository] {
        &self.repositories
    }

    /// Returns the requested maximum result count.
    #[must_use]
    pub const fn max_results(&self) -> SearchResultLimit {
        self.max_results
    }

    fn validate(&self) -> Result<(), ExternalResearchContractError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(ExternalResearchContractError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        Self::new(
            self.query.clone(),
            self.repositories.clone(),
            self.max_results,
        )
        .map(|_| ())
    }

    fn content_id(
        &self,
    ) -> Result<ContentId<ExternalTestSearchRequestArtifact>, ExternalResearchContractError> {
        let bytes = cairn_codec::to_vec(self)
            .map_err(|error| ExternalResearchContractError::Encoding(error.to_string()))?;
        ContentId::derive(&bytes)
            .map_err(|error| ExternalResearchContractError::Encoding(error.to_string()))
    }
}

impl TryFrom<ExternalTestSearchRequestWire> for ExternalTestSearchRequestV1 {
    type Error = ExternalResearchContractError;

    fn try_from(wire: ExternalTestSearchRequestWire) -> Result<Self, Self::Error> {
        if wire.schema_version != SCHEMA_V1 {
            return Err(ExternalResearchContractError::UnsupportedSchema(
                wire.schema_version,
            ));
        }
        Self::new(wire.query, wire.repositories, wire.max_results)
    }
}

impl From<ExternalTestSearchRequestV1> for ExternalTestSearchRequestWire {
    fn from(value: ExternalTestSearchRequestV1) -> Self {
        Self {
            schema_version: value.schema_version,
            query: value.query,
            repositories: value.repositories,
            max_results: value.max_results,
        }
    }
}

/// Repository license evidence. Unknown remains explicit and cannot authorize redistribution.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ExternalLicenseEvidenceV1 {
    /// Repository API declared one SPDX identifier and canonical license URL.
    RepositoryDeclared {
        /// Exact SPDX identifier returned by the provider.
        spdx_id: String,
        /// Canonical provider URL for the declaration.
        source_url: String,
    },
    /// No usable license declaration was available.
    Unknown,
}

impl ExternalLicenseEvidenceV1 {
    fn validate(&self) -> Result<(), ExternalResearchContractError> {
        match self {
            Self::RepositoryDeclared {
                spdx_id,
                source_url,
            } => {
                if spdx_id.is_empty()
                    || spdx_id.len() > 128
                    || spdx_id.chars().any(char::is_control)
                    || !source_url.starts_with("https://api.github.com/")
                {
                    return Err(ExternalResearchContractError::InvalidLicenseEvidence);
                }
                Ok(())
            }
            Self::Unknown => Ok(()),
        }
    }

    /// Returns whether source-byte redistribution remains unproven.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

/// One exact fetched upstream test proposal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "ExternalTestCaseWire", into = "ExternalTestCaseWire")]
pub struct ExternalTestCaseV1 {
    repository: GitHubRepository,
    path: SourcePath,
    blob: GitHubBlobIdentity,
    source_url: String,
    source_text: String,
    source_bytes: ContentId<ExternalTestSourceBytesArtifact>,
    license: ExternalLicenseEvidenceV1,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ExternalTestCaseWire {
    repository: GitHubRepository,
    path: SourcePath,
    blob: GitHubBlobIdentity,
    source_url: String,
    source_text: String,
    source_bytes: ContentId<ExternalTestSourceBytesArtifact>,
    license: ExternalLicenseEvidenceV1,
}

impl ExternalTestCaseV1 {
    /// Creates one fetched source proposal and derives the exact typed byte identity.
    ///
    /// # Errors
    ///
    /// Rejects empty/oversized source, inconsistent GitHub URL, or invalid license evidence.
    pub fn new(
        repository: GitHubRepository,
        path: SourcePath,
        blob: GitHubBlobIdentity,
        source_text: String,
        license: ExternalLicenseEvidenceV1,
    ) -> Result<Self, ExternalResearchContractError> {
        if source_text.is_empty() || source_text.len() > MAX_SOURCE_BYTES {
            return Err(ExternalResearchContractError::InvalidSourceBytes);
        }
        license.validate()?;
        if let ExternalLicenseEvidenceV1::RepositoryDeclared { source_url, .. } = &license {
            let expected_license_url = format!(
                "https://api.github.com/repos/{}/license",
                repository.as_str()
            );
            if source_url != &expected_license_url {
                return Err(ExternalResearchContractError::InvalidLicenseEvidence);
            }
        }
        let source_url = format!(
            "https://api.github.com/repos/{}/git/blobs/{}",
            repository.as_str(),
            blob.as_str()
        );
        let source_bytes =
            ContentId::<ExternalTestSourceBytesArtifact>::derive(source_text.as_bytes())
                .map_err(|error| ExternalResearchContractError::Encoding(error.to_string()))?;
        Ok(Self {
            repository,
            path,
            blob,
            source_url,
            source_text,
            source_bytes,
            license,
        })
    }

    /// Returns the upstream repository.
    #[must_use]
    pub const fn repository(&self) -> &GitHubRepository {
        &self.repository
    }

    /// Returns the exact upstream path.
    #[must_use]
    pub const fn path(&self) -> &SourcePath {
        &self.path
    }

    /// Returns the exact fetched source bytes as validated UTF-8.
    #[must_use]
    pub fn source_text(&self) -> &str {
        &self.source_text
    }

    /// Returns the typed identity of the exact fetched source bytes.
    #[must_use]
    pub const fn source_bytes(&self) -> ContentId<ExternalTestSourceBytesArtifact> {
        self.source_bytes
    }

    /// Returns license evidence without promoting it to policy acceptance.
    #[must_use]
    pub const fn license(&self) -> &ExternalLicenseEvidenceV1 {
        &self.license
    }

    fn validate(&self) -> Result<(), ExternalResearchContractError> {
        let expected = Self::new(
            self.repository.clone(),
            self.path.clone(),
            self.blob.clone(),
            self.source_text.clone(),
            self.license.clone(),
        )?;
        if expected.source_url != self.source_url || expected.source_bytes != self.source_bytes {
            return Err(ExternalResearchContractError::InconsistentSourceIdentity);
        }
        Ok(())
    }
}

impl TryFrom<ExternalTestCaseWire> for ExternalTestCaseV1 {
    type Error = ExternalResearchContractError;

    fn try_from(wire: ExternalTestCaseWire) -> Result<Self, Self::Error> {
        let value = Self {
            repository: wire.repository,
            path: wire.path,
            blob: wire.blob,
            source_url: wire.source_url,
            source_text: wire.source_text,
            source_bytes: wire.source_bytes,
            license: wire.license,
        };
        value.validate()?;
        Ok(value)
    }
}

impl From<ExternalTestCaseV1> for ExternalTestCaseWire {
    fn from(value: ExternalTestCaseV1) -> Self {
        Self {
            repository: value.repository,
            path: value.path,
            blob: value.blob,
            source_url: value.source_url,
            source_text: value.source_text,
            source_bytes: value.source_bytes,
            license: value.license,
        }
    }
}

/// Authoritative normalized result returned by one configured research provider.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    try_from = "ExternalTestSearchResultWire",
    into = "ExternalTestSearchResultWire"
)]
pub struct ExternalTestSearchResultV1 {
    schema_version: u16,
    request: ContentId<ExternalTestSearchRequestArtifact>,
    provider: String,
    observed_at: ObservedAtUnixMillis,
    cases: Vec<ExternalTestCaseV1>,
    omitted_results: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ExternalTestSearchResultWire {
    schema_version: u16,
    request: ContentId<ExternalTestSearchRequestArtifact>,
    provider: String,
    observed_at: ObservedAtUnixMillis,
    cases: Vec<ExternalTestCaseV1>,
    omitted_results: u64,
}

impl ExternalTestSearchResultV1 {
    /// Creates a result bound to one exact request.
    ///
    /// # Errors
    ///
    /// Rejects provider labels, result counts, or duplicate repository/path/blob triples.
    pub fn new(
        request: &ExternalTestSearchRequestV1,
        provider: String,
        observed_at: ObservedAtUnixMillis,
        cases: Vec<ExternalTestCaseV1>,
        omitted_results: u64,
    ) -> Result<Self, ExternalResearchContractError> {
        if provider.is_empty()
            || provider.trim() != provider
            || provider.len() > 128
            || provider.chars().any(char::is_control)
            || cases.len() > usize::from(request.max_results.get())
        {
            return Err(ExternalResearchContractError::InvalidSearchResult);
        }
        let mut seen = HashSet::new();
        for case in &cases {
            case.validate()?;
            let key = (
                case.repository.as_str(),
                case.path.as_str(),
                case.blob.as_str(),
            );
            if !seen.insert(key) || !request.repositories.contains(&case.repository) {
                return Err(ExternalResearchContractError::InvalidSearchResult);
            }
        }
        Ok(Self {
            schema_version: SCHEMA_V1,
            request: request.content_id()?,
            provider,
            observed_at,
            cases,
            omitted_results,
        })
    }

    /// Returns the exact request identity.
    #[must_use]
    pub const fn request(&self) -> ContentId<ExternalTestSearchRequestArtifact> {
        self.request
    }

    /// Returns every exact fetched source proposal in provider ranking order.
    #[must_use]
    pub fn cases(&self) -> &[ExternalTestCaseV1] {
        &self.cases
    }

    /// Returns how many provider results were intentionally omitted by bounds.
    #[must_use]
    pub const fn omitted_results(&self) -> u64 {
        self.omitted_results
    }

    fn validate(
        &self,
        request: &ExternalTestSearchRequestV1,
    ) -> Result<(), ExternalResearchContractError> {
        if self.schema_version != SCHEMA_V1 || self.request != request.content_id()? {
            return Err(ExternalResearchContractError::InconsistentSearchResult);
        }
        let expected = Self::new(
            request,
            self.provider.clone(),
            self.observed_at,
            self.cases.clone(),
            self.omitted_results,
        )?;
        if expected != *self {
            return Err(ExternalResearchContractError::InconsistentSearchResult);
        }
        Ok(())
    }
}

impl TryFrom<ExternalTestSearchResultWire> for ExternalTestSearchResultV1 {
    type Error = ExternalResearchContractError;

    fn try_from(wire: ExternalTestSearchResultWire) -> Result<Self, Self::Error> {
        if wire.schema_version != SCHEMA_V1 {
            return Err(ExternalResearchContractError::UnsupportedSchema(
                wire.schema_version,
            ));
        }
        if wire.provider.is_empty()
            || wire.provider.trim() != wire.provider
            || wire.provider.len() > 128
            || wire.provider.chars().any(char::is_control)
            || wire.cases.len() > usize::from(MAX_RESULTS)
        {
            return Err(ExternalResearchContractError::InvalidSearchResult);
        }
        let mut seen = HashSet::new();
        for case in &wire.cases {
            case.validate()?;
            if !seen.insert((
                case.repository.as_str(),
                case.path.as_str(),
                case.blob.as_str(),
            )) {
                return Err(ExternalResearchContractError::InvalidSearchResult);
            }
        }
        Ok(Self {
            schema_version: wire.schema_version,
            request: wire.request,
            provider: wire.provider,
            observed_at: wire.observed_at,
            cases: wire.cases,
            omitted_results: wire.omitted_results,
        })
    }
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ExternalTestProvenanceV1 {
    schema_version: u16,
    search_result: ContentId<ExternalTestSearchResultArtifact>,
    request: ContentId<ExternalTestSearchRequestArtifact>,
    repository: GitHubRepository,
    path: SourcePath,
    blob: GitHubBlobIdentity,
    source_bytes: ContentId<ExternalTestSourceBytesArtifact>,
    license: ContentId<LicenseProvenanceArtifact>,
}

/// Archived external source and separate provenance/license identities ready for a later proposal.
///
/// This value deliberately contains no `CorpusCaseArtifact`: research evidence cannot promote
/// itself into an executable case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchivedExternalTestEvidence {
    search_result: ContentId<ExternalTestSearchResultArtifact>,
    source_bytes: ContentId<ExternalTestSourceBytesArtifact>,
    provenance: ContentId<CorpusCaseProvenanceArtifact>,
    license: ContentId<LicenseProvenanceArtifact>,
    has_declared_license: bool,
}

impl ArchivedExternalTestEvidence {
    /// Returns the normalized search result containing this evidence.
    #[must_use]
    pub const fn search_result(&self) -> ContentId<ExternalTestSearchResultArtifact> {
        self.search_result
    }

    /// Returns the exact archived source-byte identity.
    #[must_use]
    pub const fn source_bytes(&self) -> ContentId<ExternalTestSourceBytesArtifact> {
        self.source_bytes
    }

    /// Returns external source provenance suitable for a later corpus proposal edge.
    #[must_use]
    pub const fn provenance(&self) -> ContentId<CorpusCaseProvenanceArtifact> {
        self.provenance
    }

    /// Returns the separately archived license evidence.
    #[must_use]
    pub const fn license(&self) -> ContentId<LicenseProvenanceArtifact> {
        self.license
    }

    /// Returns whether the provider supplied non-unknown license evidence.
    #[must_use]
    pub const fn has_declared_license(&self) -> bool {
        self.has_declared_license
    }
}

/// Archives a validated research result, exact source bytes, and separate provenance/license
/// artifacts without creating an executable corpus case.
///
/// # Errors
///
/// Rejects a result that does not bind to the exact request, any identity inconsistency, or a
/// content-store failure.
pub fn archive_external_test_evidence<S: ContentStore>(
    content: &mut S,
    request: &ExternalTestSearchRequestV1,
    result: &ExternalTestSearchResultV1,
) -> Result<Vec<ArchivedExternalTestEvidence>, ExternalResearchContractError> {
    result.validate(request)?;
    let result_bytes = cairn_codec::to_vec(result)
        .map_err(|error| ExternalResearchContractError::Encoding(error.to_string()))?;
    let result_descriptor =
        content.put::<ExternalTestSearchResultArtifact>(&mut std::io::Cursor::new(result_bytes))?;
    let mut archived = Vec::with_capacity(result.cases.len());
    for case in &result.cases {
        let source_descriptor = content.put::<ExternalTestSourceBytesArtifact>(
            &mut std::io::Cursor::new(case.source_text.as_bytes()),
        )?;
        if source_descriptor.content_id != case.source_bytes {
            return Err(ExternalResearchContractError::InconsistentSourceIdentity);
        }
        let license_bytes = cairn_codec::to_vec(&case.license)
            .map_err(|error| ExternalResearchContractError::Encoding(error.to_string()))?;
        let license_descriptor =
            content.put::<LicenseProvenanceArtifact>(&mut std::io::Cursor::new(license_bytes))?;
        let provenance = ExternalTestProvenanceV1 {
            schema_version: SCHEMA_V1,
            search_result: result_descriptor.content_id,
            request: result.request,
            repository: case.repository.clone(),
            path: case.path.clone(),
            blob: case.blob.clone(),
            source_bytes: case.source_bytes,
            license: license_descriptor.content_id,
        };
        let provenance_bytes = cairn_codec::to_vec(&provenance)
            .map_err(|error| ExternalResearchContractError::Encoding(error.to_string()))?;
        let provenance_descriptor = content
            .put::<CorpusCaseProvenanceArtifact>(&mut std::io::Cursor::new(provenance_bytes))?;
        archived.push(ArchivedExternalTestEvidence {
            search_result: result_descriptor.content_id,
            source_bytes: source_descriptor.content_id,
            provenance: provenance_descriptor.content_id,
            license: license_descriptor.content_id,
            has_declared_license: !case.license.is_unknown(),
        });
    }
    Ok(archived)
}

impl From<ExternalTestSearchResultV1> for ExternalTestSearchResultWire {
    fn from(value: ExternalTestSearchResultV1) -> Self {
        Self {
            schema_version: value.schema_version,
            request: value.request,
            provider: value.provider,
            observed_at: value.observed_at,
            cases: value.cases,
            omitted_results: value.omitted_results,
        }
    }
}

/// Operator-owned network policy enforced after model argument validation and before invocation.
#[derive(Clone, Debug)]
pub struct ExternalResearchPolicy {
    allowed_repositories: HashSet<GitHubRepository>,
    max_results: SearchResultLimit,
}

impl ExternalResearchPolicy {
    /// Creates a non-empty allowlist and a hard result ceiling.
    ///
    /// # Errors
    ///
    /// Rejects an empty allowlist.
    pub fn new(
        allowed_repositories: impl IntoIterator<Item = GitHubRepository>,
        max_results: SearchResultLimit,
    ) -> Result<Self, ExternalResearchContractError> {
        let allowed_repositories = allowed_repositories.into_iter().collect::<HashSet<_>>();
        if allowed_repositories.is_empty() || allowed_repositories.len() > MAX_REPOSITORIES {
            return Err(ExternalResearchContractError::InvalidRepositories);
        }
        Ok(Self {
            allowed_repositories,
            max_results,
        })
    }

    fn admits(&self, request: &ExternalTestSearchRequestV1) -> bool {
        request.max_results.get() <= self.max_results.get()
            && request
                .repositories
                .iter()
                .all(|repository| self.allowed_repositories.contains(repository))
    }
}

/// Replaceable external research provider. Recorded and live adapters share this seam.
pub trait ExternalResearchProvider {
    /// Executes a fully validated, policy-admitted read-only request.
    ///
    /// # Errors
    ///
    /// Returns an effect-classified provider failure.
    fn search(
        &mut self,
        request: &ExternalTestSearchRequestV1,
    ) -> Result<ExternalTestSearchResultV1, ExternalResearchProviderError>;
}

/// One exact request/result pair for deterministic offline replay.
pub struct RecordedExternalResearchExchange {
    /// Exact request identity expected by this exchange.
    pub request: ContentId<ExternalTestSearchRequestArtifact>,
    /// Previously recorded normalized result.
    pub result: ExternalTestSearchResultV1,
}

/// FIFO recorded research provider used by ordinary CI and replay.
pub struct RecordedExternalResearchProvider {
    exchanges: VecDeque<RecordedExternalResearchExchange>,
}

impl RecordedExternalResearchProvider {
    /// Creates a provider from ordered exact exchanges.
    #[must_use]
    pub fn new(exchanges: impl IntoIterator<Item = RecordedExternalResearchExchange>) -> Self {
        Self {
            exchanges: exchanges.into_iter().collect(),
        }
    }
}

impl ExternalResearchProvider for RecordedExternalResearchProvider {
    fn search(
        &mut self,
        request: &ExternalTestSearchRequestV1,
    ) -> Result<ExternalTestSearchResultV1, ExternalResearchProviderError> {
        let exchange =
            self.exchanges
                .pop_front()
                .ok_or(ExternalResearchProviderError::NotStarted(
                    "recorded research fixture is exhausted".to_owned(),
                ))?;
        let request_id = request
            .content_id()
            .map_err(|error| ExternalResearchProviderError::Rejected(error.to_string()))?;
        if exchange.request != request_id {
            return Err(ExternalResearchProviderError::NotStarted(
                "recorded research request does not match".to_owned(),
            ));
        }
        exchange
            .result
            .validate(request)
            .map_err(|error| ExternalResearchProviderError::Rejected(error.to_string()))?;
        Ok(exchange.result)
    }
}

/// Product gateway validating the blue tool boundary before and after provider access.
pub struct ExternalTestSearchGateway<P> {
    policy: ExternalResearchPolicy,
    provider: P,
}

impl<P> ExternalTestSearchGateway<P> {
    /// Creates a gateway with immutable task policy and one provider adapter.
    #[must_use]
    pub const fn new(policy: ExternalResearchPolicy, provider: P) -> Self {
        Self { policy, provider }
    }
}

impl<P: ExternalResearchProvider> ToolGateway for ExternalTestSearchGateway<P> {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        if operation.tool().as_str() != SEARCH_TOOL_NAME
            || operation.implementation_version().as_str() != SEARCH_TOOL_VERSION
            || operation.effect() != ToolEffectClass::ReadOnly
        {
            return Err(ToolGatewayError::NotStarted(
                "operation is not the trusted external-test search registration".to_owned(),
            ));
        }
        let request: ExternalTestSearchRequestV1 =
            cairn_codec::from_slice(operation.argument_bytes()).map_err(|error| {
                ToolGatewayError::Rejected(format!("invalid external-test request: {error}"))
            })?;
        let canonical = cairn_codec::to_vec(&request).map_err(|error| {
            ToolGatewayError::Rejected(format!("invalid external-test request: {error}"))
        })?;
        if canonical != operation.argument_bytes() || !self.policy.admits(&request) {
            return Err(ToolGatewayError::Rejected(
                "external-test request violates canonical task policy".to_owned(),
            ));
        }
        let result = self
            .provider
            .search(&request)
            .map_err(|error| match error {
                ExternalResearchProviderError::NotStarted(message) => {
                    ToolGatewayError::NotStarted(message)
                }
                ExternalResearchProviderError::Rejected(message) => {
                    ToolGatewayError::Rejected(message)
                }
                ExternalResearchProviderError::Ambiguous(message) => {
                    ToolGatewayError::Ambiguous(message)
                }
            })?;
        result
            .validate(&request)
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        let value = serde_json::to_value(result)
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        CanonicalToolResult::from_value(&value)
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
    }
}

/// Returns the exact trusted registration offered only to the first blue profile.
///
/// # Errors
///
/// Returns an error only if built-in labels violate the generic agent boundary.
pub fn external_test_search_registration() -> Result<ToolRegistration, ExternalResearchContractError>
{
    Ok(ToolRegistration::new(
        ToolName::new(SEARCH_TOOL_NAME)
            .map_err(|_| ExternalResearchContractError::InvalidBuiltInRegistration)?,
        ToolImplementationVersion::new(SEARCH_TOOL_VERSION)
            .map_err(|_| ExternalResearchContractError::InvalidBuiltInRegistration)?,
        ToolEffectClass::ReadOnly,
    ))
}

/// Live GitHub Search/Blob adapter. The model cannot choose its endpoint or credential.
pub struct GitHubExternalResearchProvider {
    client: Client,
    credential_file: Option<PathBuf>,
    max_response_bytes: u64,
}

impl GitHubExternalResearchProvider {
    /// Creates the fixed-authority HTTPS adapter without reading credentials.
    ///
    /// # Errors
    ///
    /// Returns an error when the bounded no-redirect client cannot be constructed.
    pub fn new(
        credential_file: Option<PathBuf>,
        max_response_bytes: u64,
    ) -> Result<Self, ExternalResearchProviderError> {
        if max_response_bytes == 0 {
            return Err(ExternalResearchProviderError::NotStarted(
                "GitHub response limit must be positive".to_owned(),
            ));
        }
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| ExternalResearchProviderError::NotStarted(error.to_string()))?;
        Ok(Self {
            client,
            credential_file,
            max_response_bytes,
        })
    }

    fn headers(&self) -> Result<HeaderMap, ExternalResearchProviderError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("cairn-oracle-research-v1"),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        if let Some(path) = &self.credential_file {
            let token = fs::read_to_string(path).map_err(|error| {
                ExternalResearchProviderError::NotStarted(format!(
                    "GitHub credential reference is unavailable: {error}"
                ))
            })?;
            let token = token.trim_end_matches(['\r', '\n']);
            if token.is_empty()
                || token.chars().any(char::is_whitespace)
                || token.chars().any(char::is_control)
            {
                return Err(ExternalResearchProviderError::NotStarted(
                    "GitHub credential reference is invalid".to_owned(),
                ));
            }
            let mut value = HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| {
                ExternalResearchProviderError::NotStarted("invalid GitHub credential".to_owned())
            })?;
            value.set_sensitive(true);
            headers.insert(AUTHORIZATION, value);
        }
        Ok(headers)
    }

    fn get_json(
        &self,
        url: &str,
        query: &[(&str, String)],
    ) -> Result<serde_json::Value, ExternalResearchProviderError> {
        self.fetch_json(url, query, false)?.ok_or_else(|| {
            ExternalResearchProviderError::Rejected(
                "GitHub resource unexpectedly disappeared".to_owned(),
            )
        })
    }

    fn fetch_json(
        &self,
        url: &str,
        query: &[(&str, String)],
        allow_not_found: bool,
    ) -> Result<Option<serde_json::Value>, ExternalResearchProviderError> {
        let response = self
            .client
            .get(url)
            .headers(self.headers()?)
            .query(query)
            .send()
            .map_err(|error| classify_research_send(&error))?;
        let status = response.status();
        let declared = response.content_length();
        let mut bytes = Vec::new();
        response
            .take(self.max_response_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| ExternalResearchProviderError::Ambiguous(error.to_string()))?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > self.max_response_bytes
            || declared.is_some_and(|length| length > self.max_response_bytes)
        {
            return Err(ExternalResearchProviderError::Rejected(
                "GitHub response exceeded configured bound".to_owned(),
            ));
        }
        if allow_not_found && status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(ExternalResearchProviderError::Rejected(format!(
                "GitHub returned HTTP {status}"
            )));
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| ExternalResearchProviderError::Rejected(error.to_string()))
    }

    fn repository_license(
        &self,
        repository: &GitHubRepository,
    ) -> Result<ExternalLicenseEvidenceV1, ExternalResearchProviderError> {
        let url = format!(
            "https://api.github.com/repos/{}/license",
            repository.as_str()
        );
        let Some(value) = self.fetch_json(&url, &[], true)? else {
            return Ok(ExternalLicenseEvidenceV1::Unknown);
        };
        let spdx = value
            .get("license")
            .and_then(|license| license.get("spdx_id"))
            .and_then(serde_json::Value::as_str);
        if matches!(spdx, None | Some("NOASSERTION")) {
            return Ok(ExternalLicenseEvidenceV1::Unknown);
        }
        Ok(ExternalLicenseEvidenceV1::RepositoryDeclared {
            spdx_id: spdx.unwrap_or_default().to_owned(),
            source_url: url,
        })
    }
}

impl ExternalResearchProvider for GitHubExternalResearchProvider {
    #[expect(
        clippy::too_many_lines,
        reason = "the live adapter keeps bounded search, immutable blob fetch, license fetch, and normalized result construction visibly sequenced"
    )]
    fn search(
        &mut self,
        request: &ExternalTestSearchRequestV1,
    ) -> Result<ExternalTestSearchResultV1, ExternalResearchProviderError> {
        request
            .validate()
            .map_err(|error| ExternalResearchProviderError::NotStarted(error.to_string()))?;
        let mut candidates = Vec::new();
        let mut total_count = 0_u64;
        for repository in &request.repositories {
            let query = format!("{} repo:{}", request.query.as_str(), repository.as_str());
            let value = self.get_json(
                "https://api.github.com/search/code",
                &[
                    ("q", query),
                    ("per_page", request.max_results.get().to_string()),
                ],
            )?;
            total_count = total_count.saturating_add(
                value
                    .get("total_count")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
            );
            let Some(items) = value.get("items").and_then(serde_json::Value::as_array) else {
                return Err(ExternalResearchProviderError::Rejected(
                    "GitHub search response has no items".to_owned(),
                ));
            };
            for item in items {
                if candidates.len() >= usize::from(request.max_results.get()) {
                    break;
                }
                let path = item
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        ExternalResearchProviderError::Rejected(
                            "GitHub result has no path".to_owned(),
                        )
                    })?;
                let sha = item
                    .get("sha")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        ExternalResearchProviderError::Rejected(
                            "GitHub result has no blob identity".to_owned(),
                        )
                    })?;
                candidates.push((
                    repository.clone(),
                    SourcePath::new(path).map_err(|error| contract_rejected(&error))?,
                    GitHubBlobIdentity::new(sha).map_err(|error| contract_rejected(&error))?,
                ));
            }
            if candidates.len() >= usize::from(request.max_results.get()) {
                break;
            }
        }

        let mut licenses: HashMap<GitHubRepository, ExternalLicenseEvidenceV1> = HashMap::new();
        let mut cases = Vec::with_capacity(candidates.len());
        for (repository, path, blob) in candidates {
            let url = format!(
                "https://api.github.com/repos/{}/git/blobs/{}",
                repository.as_str(),
                blob.as_str()
            );
            let value = self.get_json(&url, &[])?;
            let encoding = value.get("encoding").and_then(serde_json::Value::as_str);
            if encoding != Some("base64") {
                return Err(ExternalResearchProviderError::Rejected(
                    "GitHub blob is not base64 encoded".to_owned(),
                ));
            }
            let encoded = value
                .get("content")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    ExternalResearchProviderError::Rejected("GitHub blob has no content".to_owned())
                })?
                .replace(['\r', '\n'], "");
            let bytes = STANDARD
                .decode(encoded)
                .map_err(|error| ExternalResearchProviderError::Rejected(error.to_string()))?;
            if bytes.is_empty() || bytes.len() > MAX_SOURCE_BYTES {
                return Err(ExternalResearchProviderError::Rejected(
                    "GitHub source bytes violate configured bound".to_owned(),
                ));
            }
            let source = String::from_utf8(bytes).map_err(|_| {
                ExternalResearchProviderError::Rejected(
                    "GitHub test source is not UTF-8".to_owned(),
                )
            })?;
            let license = if let Some(license) = licenses.get(&repository) {
                license.clone()
            } else {
                let license = self.repository_license(&repository)?;
                licenses.insert(repository.clone(), license.clone());
                license
            };
            cases.push(
                ExternalTestCaseV1::new(repository, path, blob, source, license)
                    .map_err(|error| contract_rejected(&error))?,
            );
        }
        let observed_at = observed_now()?;
        let omitted_results = total_count.saturating_sub(cases.len() as u64);
        ExternalTestSearchResultV1::new(
            request,
            "github-code-search".to_owned(),
            observed_at,
            cases,
            omitted_results,
        )
        .map_err(|error| contract_rejected(&error))
    }
}

fn observed_now() -> Result<ObservedAtUnixMillis, ExternalResearchProviderError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| ExternalResearchProviderError::NotStarted(error.to_string()))?;
    let millis = i64::try_from(duration.as_millis()).map_err(|_| {
        ExternalResearchProviderError::NotStarted("system time cannot be represented".to_owned())
    })?;
    Ok(ObservedAtUnixMillis::new(millis))
}

fn classify_research_send(error: &reqwest::Error) -> ExternalResearchProviderError {
    if error.is_builder() || error.is_connect() {
        ExternalResearchProviderError::NotStarted(error.to_string())
    } else {
        ExternalResearchProviderError::Ambiguous(error.to_string())
    }
}

fn contract_rejected(error: &ExternalResearchContractError) -> ExternalResearchProviderError {
    ExternalResearchProviderError::Rejected(error.to_string())
}

/// Effect-classified external provider failure.
#[derive(Debug, Error)]
pub enum ExternalResearchProviderError {
    /// Network access definitely did not begin.
    #[error("external research did not start: {0}")]
    NotStarted(String),
    /// Provider or result definitively rejected the request.
    #[error("external research was rejected: {0}")]
    Rejected(String),
    /// The read-only request may have reached the provider but no result was established.
    #[error("external research outcome is ambiguous: {0}")]
    Ambiguous(String),
}

/// Strict request/result contract failure.
#[derive(Debug, Error)]
pub enum ExternalResearchContractError {
    /// Typed content archival failed.
    #[error(transparent)]
    Content(#[from] ContentStoreError),
    /// A string boundary failed validation.
    #[error("invalid external research field {0}")]
    InvalidField(&'static str),
    /// Result limit is outside V1 bounds.
    #[error("external research result limit must be between one and ten")]
    InvalidResultLimit,
    /// Repository set is empty, duplicated, unsorted, or too large.
    #[error("external research repositories are invalid")]
    InvalidRepositories,
    /// Source bytes are empty or exceed the V1 bound.
    #[error("external research source bytes are invalid")]
    InvalidSourceBytes,
    /// License evidence is malformed.
    #[error("external research license evidence is invalid")]
    InvalidLicenseEvidence,
    /// Stored source URL or identity disagrees with exact source bytes.
    #[error("external research source identity is inconsistent")]
    InconsistentSourceIdentity,
    /// Normalized search result violates request bounds or uniqueness.
    #[error("external research search result is invalid")]
    InvalidSearchResult,
    /// Stored result does not bind to its exact request or recomputed body.
    #[error("external research result is inconsistent with its request")]
    InconsistentSearchResult,
    /// A schema other than the single current V1 was supplied.
    #[error("unsupported external research schema version {0}")]
    UnsupportedSchema(u16),
    /// Built-in tool labels unexpectedly fail the generic agent boundary.
    #[error("external research built-in tool registration is invalid")]
    InvalidBuiltInRegistration,
    /// Canonical encoding or content identity failed.
    #[error("external research encoding failed: {0}")]
    Encoding(String),
}

#[cfg(test)]
mod tests {
    use cairn_agent::{
        ToolOperationCompletion, authorize_tool_operation, begin_tool_operation,
        execute_tool_operation, prepare_tool_operation,
    };
    use cairn_protocol::{AttemptId, CommandId, OperationId};
    use cairn_record::ContentStore;
    use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

    use super::ExternalResearchProvider as _;
    use super::{
        ExternalLicenseEvidenceV1, ExternalResearchPolicy, ExternalTestCaseV1,
        ExternalTestSearchGateway, ExternalTestSearchRequestV1, ExternalTestSearchResultV1,
        GitHubBlobIdentity, GitHubExternalResearchProvider, GitHubRepository,
        RecordedExternalResearchExchange, RecordedExternalResearchProvider, SearchQuery,
        SearchResultLimit, SourcePath, archive_external_test_evidence,
        external_test_search_registration,
    };

    fn request() -> ExternalTestSearchRequestV1 {
        ExternalTestSearchRequestV1::new(
            SearchQuery::new("reduction sum empty float32").expect("query"),
            vec![GitHubRepository::new("pytorch/pytorch").expect("repository")],
            SearchResultLimit::new(3).expect("limit"),
        )
        .expect("request")
    }

    fn result(
        request: &ExternalTestSearchRequestV1,
        unknown_license: bool,
    ) -> ExternalTestSearchResultV1 {
        let license = if unknown_license {
            ExternalLicenseEvidenceV1::Unknown
        } else {
            ExternalLicenseEvidenceV1::RepositoryDeclared {
                spdx_id: "BSD-3-Clause".to_owned(),
                source_url: "https://api.github.com/repos/pytorch/pytorch/license".to_owned(),
            }
        };
        let case = ExternalTestCaseV1::new(
            GitHubRepository::new("pytorch/pytorch").expect("repository"),
            SourcePath::new("test/test_reductions.py").expect("path"),
            GitHubBlobIdentity::new("0123456789abcdef0123456789abcdef01234567").expect("blob"),
            "def test_sum_empty(self):\n    assert torch.empty(0).sum() == 0\n".to_owned(),
            license,
        )
        .expect("case");
        ExternalTestSearchResultV1::new(
            request,
            "recorded-github".to_owned(),
            cairn_protocol::ObservedAtUnixMillis::new(7),
            vec![case],
            2,
        )
        .expect("result")
    }

    #[test]
    fn recorded_pytorch_search_runs_through_durable_read_only_operation() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content");
        let mut events = SqliteEventStore::in_memory().expect("events");
        let request = request();
        let request_id = request.content_id().expect("request id");
        let arguments = serde_json::to_value(&request).expect("arguments");
        let registration = external_test_search_registration().expect("registration");
        let operation = prepare_tool_operation(
            &mut content,
            OperationId::new(),
            registration.name().clone(),
            registration.implementation_version().clone(),
            registration.effect(),
            &arguments,
        )
        .expect("prepare");
        let operation_id = operation.operation_id();
        let authority = authorize_tool_operation(
            &mut events,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(1),
            operation,
        )
        .expect("authorize");
        let started = begin_tool_operation(
            &mut events,
            authority,
            AttemptId::new(),
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin");
        let provider = RecordedExternalResearchProvider::new([RecordedExternalResearchExchange {
            request: request_id,
            result: result(&request, false),
        }]);
        let policy = ExternalResearchPolicy::new(
            [GitHubRepository::new("pytorch/pytorch").expect("repository")],
            SearchResultLimit::new(3).expect("limit"),
        )
        .expect("policy");
        let mut gateway = ExternalTestSearchGateway::new(policy, provider);
        let completion = execute_tool_operation(
            &mut events,
            &mut content,
            &mut gateway,
            started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(3),
        )
        .expect("execute");
        let ToolOperationCompletion::Completed { result_id, .. } = completion else {
            panic!("expected completion");
        };
        let mut bytes = Vec::new();
        content
            .write_to(&result_id, &mut bytes)
            .expect("read result");
        let archived: ExternalTestSearchResultV1 =
            cairn_codec::from_slice(&bytes).expect("strict result");
        assert_eq!(archived.request(), request_id);
        assert_eq!(archived.cases().len(), 1);
        assert!(!archived.cases()[0].license().is_unknown());
        assert!(matches!(
            cairn_agent::recover_tool_operation(&events, operation_id).expect("recover"),
            cairn_agent::ToolOperationState::Completed { result_id: recovered, .. }
                if recovered == result_id
        ));
    }

    #[test]
    #[ignore = "opt-in live GitHub API check; set CAIRN_GITHUB_TOKEN_FILE when authentication is required"]
    fn live_github_adapter_fetches_immutable_test_bytes() {
        let credential = std::env::var_os("CAIRN_GITHUB_TOKEN_FILE").map(std::path::PathBuf::from);
        let mut provider = GitHubExternalResearchProvider::new(credential, 1_048_576)
            .expect("live GitHub provider");
        let result = provider.search(&request()).expect("live GitHub search");
        assert!(result.cases().iter().all(|case| {
            case.repository().as_str() == "pytorch/pytorch" && !case.source_text().is_empty()
        }));
    }

    #[test]
    fn unknown_license_is_visible_and_source_identity_is_recomputed() {
        let request = request();
        let result = result(&request, true);
        assert!(result.cases()[0].license().is_unknown());
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content");
        let archived =
            archive_external_test_evidence(&mut content, &request, &result).expect("archive");
        assert_eq!(archived.len(), 1);
        assert!(!archived[0].has_declared_license());
        let mut source = Vec::new();
        content
            .write_to(&archived[0].source_bytes(), &mut source)
            .expect("source bytes");
        assert_eq!(source, result.cases()[0].source_text().as_bytes());
        let bytes = cairn_codec::to_vec(&result).expect("encode");
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
        value["cases"][0]["source_text"] = serde_json::json!("changed");
        assert!(serde_json::from_value::<ExternalTestSearchResultV1>(value).is_err());
    }

    #[test]
    fn model_cannot_escape_repository_scope_or_smuggle_query_operators() {
        assert!(SearchQuery::new("sum repo:other/project").is_err());
        let request = ExternalTestSearchRequestV1::new(
            SearchQuery::new("sum").expect("query"),
            vec![GitHubRepository::new("other/project").expect("repository")],
            SearchResultLimit::new(1).expect("limit"),
        )
        .expect("request");
        let bytes = cairn_codec::to_vec(&request).expect("bytes");
        let policy = ExternalResearchPolicy::new(
            [GitHubRepository::new("pytorch/pytorch").expect("repository")],
            SearchResultLimit::new(3).expect("limit"),
        )
        .expect("policy");
        let provider = RecordedExternalResearchProvider::new([]);
        let mut gateway = ExternalTestSearchGateway::new(policy, provider);
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content");
        let registration = external_test_search_registration().expect("registration");
        let arguments: serde_json::Value = serde_json::from_slice(&bytes).expect("arguments");
        let operation = prepare_tool_operation(
            &mut content,
            OperationId::new(),
            registration.name().clone(),
            registration.implementation_version().clone(),
            registration.effect(),
            &arguments,
        )
        .expect("operation");
        assert!(cairn_agent::ToolGateway::invoke(&mut gateway, &operation).is_err());
    }

    #[test]
    fn strict_non_v1_and_unknown_fields_fail_closed() {
        let request = request();
        let bytes = cairn_codec::to_vec(&request).expect("encode");
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
        value["schema_version"] = serde_json::json!(2);
        assert!(serde_json::from_value::<ExternalTestSearchRequestV1>(value).is_err());

        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
        value["url"] = serde_json::json!("http://127.0.0.1/private");
        assert!(serde_json::from_value::<ExternalTestSearchRequestV1>(value).is_err());

        let result = result(&request, false);
        assert!(
            !cairn_codec::to_vec(&result)
                .expect("result bytes")
                .is_empty()
        );
    }
}
