//! Provider/model/deployment/protocol resolution without vendor branches in the agent loop.

use std::collections::{BTreeMap, BTreeSet};

use cairn_protocol::ContentId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    DeploymentName, ModelName, ModelProfileName, ProviderName, ResolvedRuntimeModelArtifact,
    RuntimeModelAlias,
};

const CATALOG_SCHEMA_V1: u16 = 1;

/// Invalid positive quantity or bounded sampling value in provider configuration.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ProviderConfigValueError {
    /// A required positive quantity was zero.
    #[error("provider configuration quantity must be greater than zero")]
    Zero,
    /// Temperature exceeded the portable 0.000 through 2.000 range.
    #[error("sampling temperature in millis must not exceed 2000")]
    Temperature,
}

macro_rules! positive_quantity {
    ($(#[$meta:meta])* $name:ident, $wire:ty) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(try_from = "u64", into = "u64")]
        pub struct $name($wire);

        impl $name {
            /// Creates a positive provider configuration quantity.
            ///
            /// # Errors
            ///
            /// Returns [`ProviderConfigValueError::Zero`] when `value` is zero.
            pub const fn new(value: $wire) -> Result<Self, ProviderConfigValueError> {
                if value == 0 {
                    Err(ProviderConfigValueError::Zero)
                } else {
                    Ok(Self(value))
                }
            }

            /// Returns the wire quantity.
            #[must_use]
            pub const fn get(self) -> $wire {
                self.0
            }
        }

        impl TryFrom<$wire> for $name {
            type Error = ProviderConfigValueError;

            fn try_from(value: $wire) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for $wire {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

positive_quantity!(/// Positive model context-window size in tokens.
ModelContextTokenLimit, u64);
positive_quantity!(/// Positive maximum model output size in tokens.
ModelOutputTokenLimit, u64);
positive_quantity!(/// Positive transport timeout in milliseconds.
TransportTimeoutMillis, u64);
positive_quantity!(/// Positive transport body bound in bytes.
TransportByteLimit, u64);

/// Sampling temperature represented without a raw JSON floating-point number.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct SamplingTemperatureMillis(u16);

impl SamplingTemperatureMillis {
    /// Creates a temperature from zero through 2000 inclusive.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderConfigValueError::Temperature`] above 2000.
    pub const fn new(value: u16) -> Result<Self, ProviderConfigValueError> {
        if value > 2000 {
            Err(ProviderConfigValueError::Temperature)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns thousandths of the provider temperature value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for SamplingTemperatureMillis {
    type Error = ProviderConfigValueError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<SamplingTemperatureMillis> for u16 {
    fn from(value: SamplingTemperatureMillis) -> Self {
        value.0
    }
}

/// HTTPS endpoint used by one concrete deployment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProviderEndpoint(String);

impl ProviderEndpoint {
    /// Creates a conservative HTTPS endpoint without credentials, query, or fragment.
    ///
    /// # Errors
    ///
    /// Returns [`ModelCatalogError::InvalidEndpoint`] for an unsafe or ambiguous endpoint.
    pub fn new(value: impl Into<String>) -> Result<Self, ModelCatalogError> {
        let value = value.into();
        let Some(authority_and_path) = value.strip_prefix("https://") else {
            return Err(ModelCatalogError::InvalidEndpoint(value));
        };
        let authority = authority_and_path.split('/').next().unwrap_or_default();
        if authority.is_empty()
            || authority.contains('@')
            || value.contains(['?', '#'])
            || value.chars().any(char::is_whitespace)
            || value.chars().any(char::is_control)
        {
            return Err(ModelCatalogError::InvalidEndpoint(value));
        }
        Ok(Self(value))
    }

    /// Returns the exact configured endpoint.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ProviderEndpoint {
    type Error = ModelCatalogError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ProviderEndpoint> for String {
    fn from(value: ProviderEndpoint) -> Self {
        value.0
    }
}

/// Filesystem reference to a secret whose bytes never enter the catalog or durable model snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct SecretFilePath(String);

impl SecretFilePath {
    /// Creates a non-empty normalized file reference.
    ///
    /// # Errors
    ///
    /// Returns [`ModelCatalogError::InvalidSecretPath`] for traversal, surrounding whitespace, or
    /// control characters.
    pub fn new(value: impl Into<String>) -> Result<Self, ModelCatalogError> {
        let value = value.into();
        let traverses = value.split(['/', '\\']).any(|component| component == "..");
        if value.is_empty()
            || value.trim() != value
            || value.ends_with(['/', '\\'])
            || value.chars().any(char::is_control)
            || traverses
        {
            return Err(ModelCatalogError::InvalidSecretPath(value));
        }
        Ok(Self(value))
    }

    /// Returns the configured reference without reading it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SecretFilePath {
    type Error = ModelCatalogError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<SecretFilePath> for String {
    fn from(value: SecretFilePath) -> Self {
        value.0
    }
}

/// Authentication header shape and external secret reference.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CredentialSource {
    /// `Authorization: Bearer` with bytes read from a file at dispatch time.
    BearerFile {
        /// Secret file reference.
        path: SecretFilePath,
    },
    /// `x-api-key` with bytes read from a file at dispatch time.
    XApiKeyFile {
        /// Secret file reference.
        path: SecretFilePath,
    },
}

impl CredentialSource {
    /// Returns the external secret reference.
    #[must_use]
    pub const fn path(&self) -> &SecretFilePath {
        match self {
            Self::BearerFile { path } | Self::XApiKeyFile { path } => path,
        }
    }
}

/// Provider protocol family; vendor identity is deliberately absent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ModelProtocolKind {
    /// Typed-item `OpenAI` Responses protocol.
    #[serde(rename = "openai_responses")]
    OpenAiResponses,
    /// Message-list `OpenAI` Chat Completions protocol.
    #[serde(rename = "openai_chat_completions")]
    OpenAiChatCompletions,
    /// Ordered-content-block Anthropic Messages protocol.
    #[serde(rename = "anthropic_messages")]
    AnthropicMessages,
}

/// Protocol-specific request/header settings selected by a deployment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ModelProtocolConfig {
    /// Stateless/local-continuation Responses requests.
    #[serde(rename = "openai_responses")]
    OpenAiResponses {
        /// Server-side response storage. V1 requires `false` for reconstructable local history.
        #[serde(default)]
        store: bool,
    },
    /// Stateless Chat Completions messages.
    #[serde(rename = "openai_chat_completions")]
    OpenAiChatCompletions {
        /// Emit the optional `thinking.type` extension for compatible deployments.
        #[serde(default)]
        thinking_parameter: bool,
    },
    /// Anthropic Messages with an explicit compatibility header value.
    #[serde(rename = "anthropic_messages")]
    AnthropicMessages {
        /// Value sent in `anthropic-version`.
        api_version: String,
    },
}

impl ModelProtocolConfig {
    /// Returns the protocol family.
    #[must_use]
    pub const fn kind(&self) -> ModelProtocolKind {
        match self {
            Self::OpenAiResponses { .. } => ModelProtocolKind::OpenAiResponses,
            Self::OpenAiChatCompletions { .. } => ModelProtocolKind::OpenAiChatCompletions,
            Self::AnthropicMessages { .. } => ModelProtocolKind::AnthropicMessages,
        }
    }
}

/// Declared data boundary of a deployment.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelDataBoundary {
    /// Bytes leave Cairn's trusted deployment boundary.
    ExternalProvider,
    /// Endpoint is operated inside a declared private deployment.
    PrivateDeployment,
}

/// Tool-schema dialect supported by a model profile.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSchemaDialect {
    /// General JSON Schema accepted with runtime validation.
    JsonSchema,
    /// Provider-specific strict subset, requiring schema conformance fixtures.
    StrictSubset,
}

/// Model reasoning switch represented independently from effort.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelReasoningMode {
    /// Request non-reasoning behavior where supported.
    Disabled,
    /// Request reasoning behavior where supported.
    Enabled,
}

/// Portable reasoning effort requested from a model profile.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelReasoningEffort {
    /// Low effort.
    Low,
    /// Medium effort.
    Medium,
    /// High effort.
    High,
    /// Maximum effort where supported.
    Max,
}

/// Reasoning mode and optional protocol-mapped effort.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelReasoningSettings {
    mode: ModelReasoningMode,
    #[serde(default)]
    effort: Option<ModelReasoningEffort>,
}

impl ModelReasoningSettings {
    /// Returns the requested reasoning switch.
    #[must_use]
    pub const fn mode(&self) -> ModelReasoningMode {
        self.mode
    }

    /// Returns the requested portable effort, or provider default when absent.
    #[must_use]
    pub const fn effort(&self) -> Option<ModelReasoningEffort> {
        self.effort
    }
}

/// Per-alias generation policy, separate from deployment and model-family capabilities.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelGenerationSettings {
    max_output_tokens: ModelOutputTokenLimit,
    #[serde(default)]
    temperature_millis: Option<SamplingTemperatureMillis>,
    reasoning: ModelReasoningSettings,
    #[serde(default)]
    parallel_tool_calls: bool,
}

impl ModelGenerationSettings {
    /// Returns the configured output ceiling for one provider turn.
    #[must_use]
    pub const fn max_output_tokens(&self) -> ModelOutputTokenLimit {
        self.max_output_tokens
    }

    /// Returns the configured temperature; `None` leaves it to the protocol/model default.
    #[must_use]
    pub const fn temperature_millis(&self) -> Option<SamplingTemperatureMillis> {
        self.temperature_millis
    }

    /// Returns reasoning settings.
    #[must_use]
    pub const fn reasoning(&self) -> &ModelReasoningSettings {
        &self.reasoning
    }

    /// Returns whether the request may ask the model for parallel client-tool calls.
    #[must_use]
    pub const fn parallel_tool_calls(&self) -> bool {
        self.parallel_tool_calls
    }
}

/// Transport bounds selected independently from provider protocol and model profile.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelTransportConfig {
    /// Optional connect timeout; `None` disables this timer.
    #[serde(default)]
    pub connect_timeout_ms: Option<TransportTimeoutMillis>,
    /// Optional whole-request timeout; `None` disables this timer.
    #[serde(default)]
    pub request_timeout_ms: Option<TransportTimeoutMillis>,
    /// Mandatory defense-in-depth request body bound.
    pub max_request_bytes: TransportByteLimit,
    /// Mandatory defense-in-depth response body bound.
    pub max_response_bytes: TransportByteLimit,
}

/// Alias-level choice of wire model, deployment, profile, and generation policy.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeModelConfig {
    wire_model: ModelName,
    deployment: DeploymentName,
    profile: ModelProfileName,
    settings: ModelGenerationSettings,
}

/// One HTTP endpoint, authentication scheme, protocol, and data boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ModelDeploymentConfig {
    provider: ProviderName,
    protocol: ModelProtocolConfig,
    endpoint: ProviderEndpoint,
    credential: CredentialSource,
    transport: ModelTransportConfig,
    data_boundary: ModelDataBoundary,
}

/// Declared model-family capabilities, independent from endpoint ownership.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileConfig {
    supported_protocols: BTreeSet<ModelProtocolKind>,
    supports_tools: bool,
    supports_parallel_tool_calls: bool,
    supports_reasoning: bool,
    tool_schema_dialect: ToolSchemaDialect,
    max_context_tokens: ModelContextTokenLimit,
    max_output_tokens: ModelOutputTokenLimit,
}

impl ModelProfileConfig {
    /// Returns the protocol families verified for this profile.
    #[must_use]
    pub const fn supported_protocols(&self) -> &BTreeSet<ModelProtocolKind> {
        &self.supported_protocols
    }

    /// Returns whether the model accepts client-tool definitions.
    #[must_use]
    pub const fn supports_tools(&self) -> bool {
        self.supports_tools
    }

    /// Returns whether the model accepts parallel client-tool calls.
    #[must_use]
    pub const fn supports_parallel_tool_calls(&self) -> bool {
        self.supports_parallel_tool_calls
    }

    /// Returns whether the model accepts explicit reasoning settings.
    #[must_use]
    pub const fn supports_reasoning(&self) -> bool {
        self.supports_reasoning
    }

    /// Returns the declared tool-schema dialect.
    #[must_use]
    pub const fn tool_schema_dialect(&self) -> ToolSchemaDialect {
        self.tool_schema_dialect
    }

    /// Returns the model context-window ceiling.
    #[must_use]
    pub const fn max_context_tokens(&self) -> ModelContextTokenLimit {
        self.max_context_tokens
    }

    /// Returns the model output ceiling.
    #[must_use]
    pub const fn max_output_tokens(&self) -> ModelOutputTokenLimit {
        self.max_output_tokens
    }
}

/// Strict configuration catalog. It performs no file or network I/O.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeModelCatalog {
    schema_version: u16,
    default_runtime_model: RuntimeModelAlias,
    runtime_models: BTreeMap<RuntimeModelAlias, RuntimeModelConfig>,
    deployments: BTreeMap<DeploymentName, ModelDeploymentConfig>,
    profiles: BTreeMap<ModelProfileName, ModelProfileConfig>,
}

impl RuntimeModelCatalog {
    /// Returns the catalog schema understood by this configuration.
    #[must_use]
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the alias selected when no episode override is supplied.
    #[must_use]
    pub const fn default_runtime_model(&self) -> &RuntimeModelAlias {
        &self.default_runtime_model
    }

    /// Validates all references and capability relationships without reading credentials.
    ///
    /// # Errors
    ///
    /// Returns [`ModelCatalogError`] for an unsupported schema, invalid key/reference, unsafe
    /// protocol setting, hosted state, or unsupported requested capability.
    pub fn validate(&self) -> Result<(), ModelCatalogError> {
        if self.schema_version != CATALOG_SCHEMA_V1 {
            return Err(ModelCatalogError::UnsupportedSchema(self.schema_version));
        }
        if !self
            .runtime_models
            .contains_key(&self.default_runtime_model)
        {
            return Err(ModelCatalogError::UnknownDefault(
                self.default_runtime_model.as_str().to_owned(),
            ));
        }
        if self.runtime_models.is_empty() || self.deployments.is_empty() || self.profiles.is_empty()
        {
            return Err(ModelCatalogError::EmptyCatalog);
        }

        for (name, deployment) in &self.deployments {
            validate_deployment(name, deployment)?;
        }
        for (name, profile) in &self.profiles {
            if profile.supported_protocols.is_empty() {
                return Err(ModelCatalogError::EmptyProtocolSet(
                    name.as_str().to_owned(),
                ));
            }
            if profile.max_output_tokens.get() > profile.max_context_tokens.get() {
                return Err(ModelCatalogError::InvalidProfileBounds(
                    name.as_str().to_owned(),
                ));
            }
        }
        for (alias, runtime) in &self.runtime_models {
            let deployment = self.deployments.get(&runtime.deployment).ok_or_else(|| {
                ModelCatalogError::UnknownDeployment {
                    alias: alias.as_str().to_owned(),
                    deployment: runtime.deployment.as_str().to_owned(),
                }
            })?;
            let profile = self.profiles.get(&runtime.profile).ok_or_else(|| {
                ModelCatalogError::UnknownProfile {
                    alias: alias.as_str().to_owned(),
                    profile: runtime.profile.as_str().to_owned(),
                }
            })?;
            validate_runtime(alias, runtime, deployment, profile)?;
        }
        Ok(())
    }

    /// Resolves an alias into the complete immutable, secret-free episode snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`ModelCatalogError`] when the catalog is invalid or the alias is unknown.
    pub fn resolve(
        &self,
        alias: Option<&RuntimeModelAlias>,
    ) -> Result<ResolvedRuntimeModel, ModelCatalogError> {
        self.validate()?;
        let alias = alias.unwrap_or(&self.default_runtime_model);
        let runtime = self
            .runtime_models
            .get(alias)
            .ok_or_else(|| ModelCatalogError::UnknownRuntimeModel(alias.as_str().to_owned()))?;
        let deployment = self.deployments.get(&runtime.deployment).ok_or_else(|| {
            ModelCatalogError::UnknownDeployment {
                alias: alias.as_str().to_owned(),
                deployment: runtime.deployment.as_str().to_owned(),
            }
        })?;
        let profile = self.profiles.get(&runtime.profile).ok_or_else(|| {
            ModelCatalogError::UnknownProfile {
                alias: alias.as_str().to_owned(),
                profile: runtime.profile.as_str().to_owned(),
            }
        })?;
        Ok(ResolvedRuntimeModel {
            alias: alias.clone(),
            wire_model: runtime.wire_model.clone(),
            deployment: runtime.deployment.clone(),
            profile_name: runtime.profile.clone(),
            provider: deployment.provider.clone(),
            protocol: deployment.protocol.clone(),
            endpoint: deployment.endpoint.clone(),
            credential: deployment.credential.clone(),
            transport: deployment.transport.clone(),
            data_boundary: deployment.data_boundary,
            settings: runtime.settings.clone(),
            profile: profile.clone(),
        })
    }
}

/// Fully resolved model/deployment/protocol snapshot frozen for an episode.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedRuntimeModel {
    alias: RuntimeModelAlias,
    wire_model: ModelName,
    deployment: DeploymentName,
    profile_name: ModelProfileName,
    provider: ProviderName,
    protocol: ModelProtocolConfig,
    endpoint: ProviderEndpoint,
    credential: CredentialSource,
    transport: ModelTransportConfig,
    data_boundary: ModelDataBoundary,
    settings: ModelGenerationSettings,
    profile: ModelProfileConfig,
}

impl ResolvedRuntimeModel {
    /// Returns the operator-facing alias.
    #[must_use]
    pub const fn alias(&self) -> &RuntimeModelAlias {
        &self.alias
    }

    /// Returns the provider-visible model string.
    #[must_use]
    pub const fn wire_model(&self) -> &ModelName {
        &self.wire_model
    }

    /// Returns the selected deployment.
    #[must_use]
    pub const fn deployment(&self) -> &DeploymentName {
        &self.deployment
    }

    /// Returns the selected capability profile name.
    #[must_use]
    pub const fn profile_name(&self) -> &ModelProfileName {
        &self.profile_name
    }

    /// Returns the endpoint owner label. It does not select a codec.
    #[must_use]
    pub const fn provider(&self) -> &ProviderName {
        &self.provider
    }

    /// Returns the protocol configuration that selects the codec.
    #[must_use]
    pub const fn protocol(&self) -> &ModelProtocolConfig {
        &self.protocol
    }

    /// Returns the deployment endpoint.
    #[must_use]
    pub const fn endpoint(&self) -> &ProviderEndpoint {
        &self.endpoint
    }

    /// Returns the credential reference, never credential bytes.
    #[must_use]
    pub const fn credential(&self) -> &CredentialSource {
        &self.credential
    }

    /// Returns transport bounds.
    #[must_use]
    pub const fn transport(&self) -> &ModelTransportConfig {
        &self.transport
    }

    /// Returns the declared data boundary.
    #[must_use]
    pub const fn data_boundary(&self) -> ModelDataBoundary {
        self.data_boundary
    }

    /// Returns per-alias generation settings.
    #[must_use]
    pub const fn settings(&self) -> &ModelGenerationSettings {
        &self.settings
    }

    /// Returns the resolved capability profile.
    #[must_use]
    pub const fn profile(&self) -> &ModelProfileConfig {
        &self.profile
    }

    /// Encodes the secret-free frozen snapshot using Cairn's canonical codec.
    ///
    /// # Errors
    ///
    /// Returns [`ModelCatalogError::Encoding`] when canonical encoding fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ModelCatalogError> {
        cairn_codec::to_vec(self).map_err(|error| ModelCatalogError::Encoding(error.to_string()))
    }

    /// Derives the typed identity of the frozen snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`ModelCatalogError::Encoding`] when canonical encoding or identity derivation
    /// fails.
    pub fn content_id(&self) -> Result<ContentId<ResolvedRuntimeModelArtifact>, ModelCatalogError> {
        let bytes = self.canonical_bytes()?;
        ContentId::derive(&bytes).map_err(|error| ModelCatalogError::Encoding(error.to_string()))
    }
}

fn validate_deployment(
    name: &DeploymentName,
    deployment: &ModelDeploymentConfig,
) -> Result<(), ModelCatalogError> {
    if matches!(
        deployment.protocol,
        ModelProtocolConfig::OpenAiResponses { store: true }
    ) {
        return Err(ModelCatalogError::HostedStateUnsupported(
            name.as_str().to_owned(),
        ));
    }
    if let ModelProtocolConfig::AnthropicMessages { api_version } = &deployment.protocol {
        if api_version.is_empty()
            || api_version.trim() != api_version
            || api_version.chars().any(char::is_control)
        {
            return Err(ModelCatalogError::InvalidApiVersion(
                name.as_str().to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_runtime(
    alias: &RuntimeModelAlias,
    runtime: &RuntimeModelConfig,
    deployment: &ModelDeploymentConfig,
    profile: &ModelProfileConfig,
) -> Result<(), ModelCatalogError> {
    let protocol = deployment.protocol.kind();
    if !profile.supported_protocols.contains(&protocol) {
        return Err(ModelCatalogError::UnsupportedProtocol {
            alias: alias.as_str().to_owned(),
            protocol,
        });
    }
    if runtime.settings.max_output_tokens.get() > profile.max_output_tokens.get() {
        return Err(ModelCatalogError::OutputLimitExceeded(
            alias.as_str().to_owned(),
        ));
    }
    if runtime.settings.parallel_tool_calls && !profile.supports_parallel_tool_calls {
        return Err(ModelCatalogError::ParallelToolsUnsupported(
            alias.as_str().to_owned(),
        ));
    }
    if runtime.settings.parallel_tool_calls && !profile.supports_tools {
        return Err(ModelCatalogError::ToolsUnsupported(
            alias.as_str().to_owned(),
        ));
    }
    match (
        runtime.settings.reasoning.mode,
        runtime.settings.reasoning.effort,
    ) {
        (ModelReasoningMode::Disabled, Some(_)) => {
            return Err(ModelCatalogError::DisabledReasoningHasEffort(
                alias.as_str().to_owned(),
            ));
        }
        (ModelReasoningMode::Enabled, _) if !profile.supports_reasoning => {
            return Err(ModelCatalogError::ReasoningUnsupported(
                alias.as_str().to_owned(),
            ));
        }
        _ => {}
    }
    Ok(())
}

/// Invalid runtime-model catalog or resolution request.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ModelCatalogError {
    /// Catalog schema is not supported by this build.
    #[error("unsupported runtime model catalog schema {0}")]
    UnsupportedSchema(u16),
    /// One of the required catalog maps is empty.
    #[error("runtime model catalog maps must not be empty")]
    EmptyCatalog,
    /// Default alias does not exist.
    #[error("unknown default runtime model {0}")]
    UnknownDefault(String),
    /// Requested alias does not exist.
    #[error("unknown runtime model {0}")]
    UnknownRuntimeModel(String),
    /// Alias references an absent deployment.
    #[error("runtime model {alias} references unknown deployment {deployment}")]
    UnknownDeployment {
        /// Runtime-model alias.
        alias: String,
        /// Missing deployment name.
        deployment: String,
    },
    /// Alias references an absent capability profile.
    #[error("runtime model {alias} references unknown profile {profile}")]
    UnknownProfile {
        /// Runtime-model alias.
        alias: String,
        /// Missing profile name.
        profile: String,
    },
    /// Configured HTTPS endpoint is unsafe or ambiguous.
    #[error("invalid provider HTTPS endpoint {0}")]
    InvalidEndpoint(String),
    /// Secret reference is empty, traversing, or otherwise ambiguous.
    #[error("invalid provider secret file path {0}")]
    InvalidSecretPath(String),
    /// V1 local-replay mode forbids hosted provider continuation.
    #[error("deployment {0} enables unsupported hosted response state")]
    HostedStateUnsupported(String),
    /// Anthropic version header is not a stable label.
    #[error("deployment {0} has an invalid Anthropic API version")]
    InvalidApiVersion(String),
    /// Profile supports no protocol.
    #[error("model profile {0} supports no protocol")]
    EmptyProtocolSet(String),
    /// Profile output ceiling exceeds its context window.
    #[error("model profile {0} has invalid token bounds")]
    InvalidProfileBounds(String),
    /// Deployment protocol is outside the selected profile.
    #[error("runtime model {alias} profile does not support {protocol:?}")]
    UnsupportedProtocol {
        /// Runtime-model alias.
        alias: String,
        /// Requested deployment protocol.
        protocol: ModelProtocolKind,
    },
    /// Alias output setting exceeds profile capability.
    #[error("runtime model {0} output limit exceeds its profile")]
    OutputLimitExceeded(String),
    /// Alias requests parallel tools from a profile without that capability.
    #[error("runtime model {0} requests unsupported parallel tool calls")]
    ParallelToolsUnsupported(String),
    /// Alias requests tools from a model-only profile.
    #[error("runtime model {0} requests unsupported tools")]
    ToolsUnsupported(String),
    /// Alias requests reasoning from a profile without that capability.
    #[error("runtime model {0} requests unsupported reasoning")]
    ReasoningUnsupported(String),
    /// Disabled reasoning cannot carry an effort value.
    #[error("runtime model {0} configures effort while reasoning is disabled")]
    DisabledReasoningHasEffort(String),
    /// Frozen snapshot could not be encoded or identified.
    #[error("resolved runtime model encoding failed: {0}")]
    Encoding(String),
}

#[cfg(test)]
mod tests {
    use super::{
        CredentialSource, ModelCatalogError, ModelOutputTokenLimit, ModelProtocolConfig,
        ModelProtocolKind, ProviderConfigValueError, ProviderEndpoint, RuntimeModelCatalog,
        SamplingTemperatureMillis, SecretFilePath,
    };
    use crate::{ModelName, RuntimeModelAlias};

    fn example() -> RuntimeModelCatalog {
        let bytes = cairn_codec::canonicalize(include_bytes!(
            "../../../config/runtime-models.example.json"
        ))
        .expect("canonicalized example catalog");
        cairn_codec::from_slice(&bytes).expect("typed example catalog")
    }

    fn example_value() -> serde_json::Value {
        let bytes = cairn_codec::canonicalize(include_bytes!(
            "../../../config/runtime-models.example.json"
        ))
        .expect("canonicalized example catalog");
        cairn_codec::from_slice(&bytes).expect("example catalog value")
    }

    #[test]
    fn deepseek_default_resolves_to_a_secret_free_frozen_snapshot() {
        let catalog = example();
        catalog.validate().expect("valid catalog");
        let resolved = catalog.resolve(None).expect("default model");
        assert_eq!(
            resolved.wire_model(),
            &ModelName::new("deepseek-v4-pro").unwrap()
        );
        assert_eq!(
            resolved.protocol().kind(),
            ModelProtocolKind::AnthropicMessages
        );
        assert!(matches!(
            resolved.credential(),
            CredentialSource::XApiKeyFile { .. }
        ));
        assert_eq!(
            resolved.credential().path().as_str(),
            ".cairn/secrets/deepseek-api-key"
        );
        let bytes = resolved.canonical_bytes().expect("snapshot bytes");
        let text = std::str::from_utf8(&bytes).expect("UTF-8");
        assert!(!text.contains("sk-"));
        assert_eq!(
            resolved.content_id().unwrap(),
            resolved.content_id().unwrap()
        );
    }

    #[test]
    fn a_fixture_wire_model_can_resolve_through_all_three_protocol_families() {
        let mut value = example_value();
        value["profiles"]["fixture-model"] = value["profiles"]["deepseek-v4-pro"].clone();
        value["profiles"]["fixture-model"]["supported_protocols"] = serde_json::json!([
            "anthropic_messages",
            "openai_chat_completions",
            "openai_responses"
        ]);

        for (alias, deployment) in [
            ("fixture-anthropic", "deepseek-anthropic"),
            ("fixture-chat", "deepseek-chat"),
        ] {
            value["deployments"][alias] = value["deployments"][deployment].clone();
            value["deployments"][alias]["provider"] = serde_json::json!("fixture-provider");
        }
        value["deployments"]["fixture-responses"] = value["deployments"]["fixture-chat"].clone();
        value["deployments"]["fixture-responses"]["endpoint"] =
            serde_json::json!("https://api.example.test/v1/responses");
        value["deployments"]["fixture-responses"]["protocol"] =
            serde_json::json!({"kind":"openai_responses","store":false});

        let base_runtime = value["runtime_models"]["deepseek-v4-pro-default"].clone();
        for (alias, deployment) in [
            ("fixture-anthropic", "fixture-anthropic"),
            ("fixture-chat", "fixture-chat"),
            ("fixture-responses", "fixture-responses"),
        ] {
            value["runtime_models"][alias] = base_runtime.clone();
            value["runtime_models"][alias]["deployment"] = serde_json::json!(deployment);
            value["runtime_models"][alias]["profile"] = serde_json::json!("fixture-model");
            value["runtime_models"][alias]["wire_model"] = serde_json::json!("fixture-model");
        }
        let bytes = cairn_codec::to_vec(&value).expect("canonical catalog");
        let catalog: RuntimeModelCatalog = cairn_codec::from_slice(&bytes).expect("typed catalog");

        let cases = [
            ("fixture-anthropic", ModelProtocolKind::AnthropicMessages),
            ("fixture-responses", ModelProtocolKind::OpenAiResponses),
            ("fixture-chat", ModelProtocolKind::OpenAiChatCompletions),
        ];
        for (alias, protocol) in cases {
            let alias = RuntimeModelAlias::new(alias).expect("alias");
            let resolved = catalog.resolve(Some(&alias)).expect("resolved protocol");
            assert_eq!(resolved.protocol().kind(), protocol);
            assert_eq!(
                resolved.wire_model(),
                &ModelName::new("fixture-model").unwrap()
            );
        }
    }

    #[test]
    fn catalog_rejects_hosted_state_and_inline_secrets() {
        let mut value = example_value();
        value["deployments"]["deepseek-anthropic"]["protocol"] =
            serde_json::json!({"kind":"openai_responses","store":true});
        let bytes = cairn_codec::to_vec(&value).unwrap();
        let catalog: RuntimeModelCatalog = cairn_codec::from_slice(&bytes).unwrap();
        assert!(matches!(
            catalog.validate(),
            Err(ModelCatalogError::HostedStateUnsupported(_))
        ));

        let mut value = example_value();
        value["deployments"]["deepseek-anthropic"]["credential"] =
            serde_json::json!({"kind":"x_api_key_file","api_key":"sk-secret"});
        let bytes = cairn_codec::to_vec(&value).unwrap();
        assert!(cairn_codec::from_slice::<RuntimeModelCatalog>(&bytes).is_err());
    }

    #[test]
    fn protocol_is_selected_by_configuration_not_provider_label() {
        let mut catalog = example();
        catalog
            .deployments
            .get_mut("deepseek-anthropic")
            .unwrap()
            .provider = crate::ProviderName::new("not-a-vendor-branch").unwrap();
        catalog
            .deployments
            .get_mut("deepseek-anthropic")
            .unwrap()
            .credential = CredentialSource::BearerFile {
            path: SecretFilePath::new(".cairn/secrets/gateway-key").unwrap(),
        };
        let resolved = catalog.resolve(None).expect("resolved");
        assert!(matches!(
            resolved.protocol(),
            ModelProtocolConfig::AnthropicMessages { .. }
        ));
    }

    #[test]
    fn invalid_keys_endpoints_paths_and_quantities_fail_at_typed_boundaries() {
        assert!(ProviderEndpoint::new("http://api.example.test/v1").is_err());
        assert!(SecretFilePath::new("../secret").is_err());
        assert_eq!(
            ModelOutputTokenLimit::new(0),
            Err(ProviderConfigValueError::Zero)
        );
        assert_eq!(
            SamplingTemperatureMillis::new(2001),
            Err(ProviderConfigValueError::Temperature)
        );

        let mut value = example_value();
        value["runtime_models"][" invalid-alias"] =
            value["runtime_models"]["deepseek-v4-pro-default"].clone();
        let bytes = cairn_codec::to_vec(&value).expect("canonical invalid catalog");
        assert!(cairn_codec::from_slice::<RuntimeModelCatalog>(&bytes).is_err());
    }
}
