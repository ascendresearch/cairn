//! Model-template and user-deployment resolution without vendor branches in the agent loop.

use std::collections::{BTreeMap, BTreeSet};

use cairn_protocol::ContentId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    DeploymentName, ModelName, ModelTemplateArtifact, ModelTemplateName, ProviderName,
    ResolvedRuntimeModelArtifact, RuntimeModelAlias,
};

const MODEL_TEMPLATE_SCHEMA_V1: u16 = 1;
const RUNTIME_CATALOG_SCHEMA_V1: u16 = 1;

/// Invalid positive quantity or bounded sampling value in model configuration.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ProviderConfigValueError {
    /// A required positive quantity was zero.
    #[error("model/provider configuration quantity must be greater than zero")]
    Zero,
    /// Temperature exceeded the portable 0.000 through 2.000 range.
    #[error("sampling temperature in millis must not exceed 2000")]
    Temperature,
}

macro_rules! positive_quantity {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(try_from = "u64", into = "u64")]
        pub struct $name(u64);

        impl $name {
            /// Creates a positive configuration quantity.
            ///
            /// # Errors
            ///
            /// Returns [`ProviderConfigValueError::Zero`] when `value` is zero.
            pub const fn new(value: u64) -> Result<Self, ProviderConfigValueError> {
                if value == 0 {
                    Err(ProviderConfigValueError::Zero)
                } else {
                    Ok(Self(value))
                }
            }

            /// Returns the wire quantity.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl TryFrom<u64> for $name {
            type Error = ProviderConfigValueError;

            fn try_from(value: u64) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for u64 {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

positive_quantity!(/// Positive model context-window size in tokens.
ModelContextTokenLimit);
positive_quantity!(/// Positive maximum model output size in tokens.
ModelOutputTokenLimit);
positive_quantity!(/// Positive transport timeout in milliseconds.
TransportTimeoutMillis);
positive_quantity!(/// Positive transport body bound in bytes.
TransportByteLimit);

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

/// HTTPS endpoint used by one user-configured deployment.
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

/// Filesystem reference to a secret whose bytes never enter durable configuration.
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

/// Protocol-specific model behavior supplied by a built-in template.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ModelProtocolConfig {
    /// Stateless/local-continuation Responses requests.
    #[serde(rename = "openai_responses")]
    OpenAiResponses {
        /// Server-side response storage. V1 templates require `false` for local reconstruction.
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
    /// Anthropic Messages with a model-compatible default header value.
    #[serde(rename = "anthropic_messages")]
    AnthropicMessages {
        /// Value sent in `anthropic-version` unless transport policy later overrides it.
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

/// Tool-schema dialect supported by one model/protocol combination.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSchemaDialect {
    /// General JSON Schema accepted with runtime validation.
    JsonSchema,
    /// Model-specific strict subset, requiring schema conformance fixtures.
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

/// Portable reasoning effort mapped by a protocol template.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
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

    /// Returns the requested portable effort, or model default when absent.
    #[must_use]
    pub const fn effort(&self) -> Option<ModelReasoningEffort> {
        self.effort
    }
}

/// Fully materialized generation policy for one runtime alias.
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

    /// Returns the configured temperature; `None` leaves it to the model default.
    #[must_use]
    pub const fn temperature_millis(&self) -> Option<SamplingTemperatureMillis> {
        self.temperature_millis
    }

    /// Returns reasoning settings.
    #[must_use]
    pub const fn reasoning(&self) -> &ModelReasoningSettings {
        &self.reasoning
    }

    /// Returns whether the request may ask for parallel client-tool calls.
    #[must_use]
    pub const fn parallel_tool_calls(&self) -> bool {
        self.parallel_tool_calls
    }
}

/// Optional user overrides applied within a template's declared capability bounds.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelGenerationOverrides {
    #[serde(default)]
    max_output_tokens: Option<ModelOutputTokenLimit>,
    #[serde(default)]
    temperature_millis: Option<SamplingTemperatureMillis>,
    #[serde(default)]
    reasoning: Option<ModelReasoningSettings>,
    #[serde(default)]
    parallel_tool_calls: Option<bool>,
}

/// Transport bounds selected independently from model characteristics.
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

/// Capabilities inherent to one model through one protocol family.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProtocolCapabilities {
    supports_tools: bool,
    supports_parallel_tool_calls: bool,
    supports_reasoning: bool,
    #[serde(default)]
    reasoning_efforts: BTreeSet<ModelReasoningEffort>,
    tool_schema_dialect: ToolSchemaDialect,
    max_context_tokens: ModelContextTokenLimit,
    max_output_tokens: ModelOutputTokenLimit,
}

impl ModelProtocolCapabilities {
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

    /// Returns the explicit reasoning efforts accepted by this protocol mapping.
    #[must_use]
    pub const fn reasoning_efforts(&self) -> &BTreeSet<ModelReasoningEffort> {
        &self.reasoning_efforts
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

/// One protocol-specific section inside a versioned model template.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProtocolTemplate {
    protocol: ModelProtocolConfig,
    capabilities: ModelProtocolCapabilities,
    defaults: ModelGenerationSettings,
}

impl ModelProtocolTemplate {
    /// Returns protocol wire settings.
    #[must_use]
    pub const fn protocol(&self) -> &ModelProtocolConfig {
        &self.protocol
    }

    /// Returns model capabilities for this protocol.
    #[must_use]
    pub const fn capabilities(&self) -> &ModelProtocolCapabilities {
        &self.capabilities
    }

    /// Returns model defaults for this protocol.
    #[must_use]
    pub const fn defaults(&self) -> &ModelGenerationSettings {
        &self.defaults
    }
}

/// Repository-maintained model characteristics, normally loaded from one template file.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelTemplate {
    schema_version: u16,
    name: ModelTemplateName,
    wire_model: ModelName,
    protocols: Vec<ModelProtocolTemplate>,
}

impl ModelTemplate {
    /// Returns the stable template name.
    #[must_use]
    pub const fn name(&self) -> &ModelTemplateName {
        &self.name
    }

    /// Returns the model string placed on the wire.
    #[must_use]
    pub const fn wire_model(&self) -> &ModelName {
        &self.wire_model
    }

    /// Returns all protocol-specific model sections.
    #[must_use]
    pub fn protocols(&self) -> &[ModelProtocolTemplate] {
        &self.protocols
    }

    fn protocol(&self, kind: ModelProtocolKind) -> Option<&ModelProtocolTemplate> {
        self.protocols
            .iter()
            .find(|candidate| candidate.protocol.kind() == kind)
    }

    /// Validates template-internal uniqueness and capability/default relationships.
    ///
    /// # Errors
    ///
    /// Returns [`ModelCatalogError`] for unsupported schema, duplicate protocol sections, or
    /// defaults outside declared model capabilities.
    pub fn validate(&self) -> Result<(), ModelCatalogError> {
        if self.schema_version != MODEL_TEMPLATE_SCHEMA_V1 {
            return Err(ModelCatalogError::UnsupportedTemplateSchema {
                template: self.name.as_str().to_owned(),
                schema: self.schema_version,
            });
        }
        if self.protocols.is_empty() {
            return Err(ModelCatalogError::EmptyTemplateProtocols(
                self.name.as_str().to_owned(),
            ));
        }
        let mut kinds = BTreeSet::new();
        for protocol in &self.protocols {
            let kind = protocol.protocol.kind();
            if !kinds.insert(kind) {
                return Err(ModelCatalogError::DuplicateTemplateProtocol {
                    template: self.name.as_str().to_owned(),
                    protocol: kind,
                });
            }
            validate_protocol_config(self.name.as_str(), &protocol.protocol)?;
            validate_capabilities(self.name.as_str(), &protocol.capabilities)?;
            validate_generation(
                self.name.as_str(),
                &protocol.defaults,
                &protocol.capabilities,
            )?;
        }
        Ok(())
    }

    /// Derives the typed identity of this exact template revision.
    ///
    /// # Errors
    ///
    /// Returns [`ModelCatalogError::Encoding`] when canonical encoding or identity derivation
    /// fails.
    pub fn content_id(&self) -> Result<ContentId<ModelTemplateArtifact>, ModelCatalogError> {
        let bytes = cairn_codec::to_vec(self)
            .map_err(|error| ModelCatalogError::Encoding(error.to_string()))?;
        ContentId::derive(&bytes).map_err(|error| ModelCatalogError::Encoding(error.to_string()))
    }
}

/// Validated in-memory index assembled from repository model-template files.
#[derive(Clone, Debug, Default)]
pub struct ModelTemplateRegistry {
    templates: BTreeMap<ModelTemplateName, ModelTemplate>,
}

impl ModelTemplateRegistry {
    /// Builds a validated registry from independently decoded template files.
    ///
    /// # Errors
    ///
    /// Returns [`ModelCatalogError`] when a template is invalid or a name is duplicated.
    pub fn from_templates(
        templates: impl IntoIterator<Item = ModelTemplate>,
    ) -> Result<Self, ModelCatalogError> {
        let mut registry = Self::default();
        for template in templates {
            template.validate()?;
            let name = template.name.clone();
            if registry.templates.insert(name.clone(), template).is_some() {
                return Err(ModelCatalogError::DuplicateTemplate(
                    name.as_str().to_owned(),
                ));
            }
        }
        if registry.templates.is_empty() {
            return Err(ModelCatalogError::EmptyTemplateRegistry);
        }
        Ok(registry)
    }

    /// Returns a registered template.
    #[must_use]
    pub fn get(&self, name: &ModelTemplateName) -> Option<&ModelTemplate> {
        self.templates.get(name)
    }
}

/// Alias-level user choice of model template, deployment, and optional preference overrides.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeModelConfig {
    template: ModelTemplateName,
    deployment: DeploymentName,
    #[serde(default)]
    settings: ModelGenerationOverrides,
}

/// User-owned endpoint, authentication, protocol selection, and data boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ModelDeploymentConfig {
    provider: ProviderName,
    protocol: ModelProtocolKind,
    endpoint: ProviderEndpoint,
    credential: CredentialSource,
    transport: ModelTransportConfig,
    data_boundary: ModelDataBoundary,
}

/// Strict user runtime catalog. Model capabilities are deliberately absent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeModelCatalog {
    schema_version: u16,
    default_runtime_model: RuntimeModelAlias,
    runtime_models: BTreeMap<RuntimeModelAlias, RuntimeModelConfig>,
    deployments: BTreeMap<DeploymentName, ModelDeploymentConfig>,
}

impl RuntimeModelCatalog {
    /// Returns the runtime-catalog schema.
    #[must_use]
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the alias selected when no episode override is supplied.
    #[must_use]
    pub const fn default_runtime_model(&self) -> &RuntimeModelAlias {
        &self.default_runtime_model
    }

    /// Validates user references against repository model templates without reading credentials.
    ///
    /// # Errors
    ///
    /// Returns [`ModelCatalogError`] for unsupported schema, invalid references, unsupported model
    /// protocol, or preference overrides outside template bounds.
    pub fn validate(&self, templates: &ModelTemplateRegistry) -> Result<(), ModelCatalogError> {
        if self.schema_version != RUNTIME_CATALOG_SCHEMA_V1 {
            return Err(ModelCatalogError::UnsupportedRuntimeSchema(
                self.schema_version,
            ));
        }
        if !self
            .runtime_models
            .contains_key(&self.default_runtime_model)
        {
            return Err(ModelCatalogError::UnknownDefault(
                self.default_runtime_model.as_str().to_owned(),
            ));
        }
        if self.runtime_models.is_empty() || self.deployments.is_empty() {
            return Err(ModelCatalogError::EmptyRuntimeCatalog);
        }
        for (alias, runtime) in &self.runtime_models {
            let deployment = self.deployments.get(&runtime.deployment).ok_or_else(|| {
                ModelCatalogError::UnknownDeployment {
                    alias: alias.as_str().to_owned(),
                    deployment: runtime.deployment.as_str().to_owned(),
                }
            })?;
            let template = templates.get(&runtime.template).ok_or_else(|| {
                ModelCatalogError::UnknownTemplate {
                    alias: alias.as_str().to_owned(),
                    template: runtime.template.as_str().to_owned(),
                }
            })?;
            let protocol = template.protocol(deployment.protocol).ok_or_else(|| {
                ModelCatalogError::UnsupportedProtocol {
                    alias: alias.as_str().to_owned(),
                    protocol: deployment.protocol,
                }
            })?;
            let settings = merge_settings(&protocol.defaults, &runtime.settings);
            validate_generation(alias.as_str(), &settings, &protocol.capabilities)?;
        }
        Ok(())
    }

    /// Resolves a user alias and repository template into a frozen secret-free episode snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`ModelCatalogError`] when either catalog is invalid or the alias is unknown.
    pub fn resolve(
        &self,
        templates: &ModelTemplateRegistry,
        alias: Option<&RuntimeModelAlias>,
    ) -> Result<ResolvedRuntimeModel, ModelCatalogError> {
        self.validate(templates)?;
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
        let template =
            templates
                .get(&runtime.template)
                .ok_or_else(|| ModelCatalogError::UnknownTemplate {
                    alias: alias.as_str().to_owned(),
                    template: runtime.template.as_str().to_owned(),
                })?;
        let protocol = template.protocol(deployment.protocol).ok_or_else(|| {
            ModelCatalogError::UnsupportedProtocol {
                alias: alias.as_str().to_owned(),
                protocol: deployment.protocol,
            }
        })?;
        Ok(ResolvedRuntimeModel {
            alias: alias.clone(),
            template_name: template.name.clone(),
            template_id: template.content_id()?,
            wire_model: template.wire_model.clone(),
            deployment: runtime.deployment.clone(),
            provider: deployment.provider.clone(),
            protocol: protocol.protocol.clone(),
            endpoint: deployment.endpoint.clone(),
            credential: deployment.credential.clone(),
            transport: deployment.transport.clone(),
            data_boundary: deployment.data_boundary,
            settings: merge_settings(&protocol.defaults, &runtime.settings),
            capabilities: protocol.capabilities.clone(),
        })
    }
}

/// Fully resolved model/template/deployment/protocol snapshot frozen for an episode.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedRuntimeModel {
    alias: RuntimeModelAlias,
    template_name: ModelTemplateName,
    template_id: ContentId<ModelTemplateArtifact>,
    wire_model: ModelName,
    deployment: DeploymentName,
    provider: ProviderName,
    protocol: ModelProtocolConfig,
    endpoint: ProviderEndpoint,
    credential: CredentialSource,
    transport: ModelTransportConfig,
    data_boundary: ModelDataBoundary,
    settings: ModelGenerationSettings,
    capabilities: ModelProtocolCapabilities,
}

impl ResolvedRuntimeModel {
    /// Returns the operator-facing alias.
    #[must_use]
    pub const fn alias(&self) -> &RuntimeModelAlias {
        &self.alias
    }

    /// Returns the selected repository template name.
    #[must_use]
    pub const fn template_name(&self) -> &ModelTemplateName {
        &self.template_name
    }

    /// Returns the exact selected template revision identity.
    #[must_use]
    pub const fn template_id(&self) -> ContentId<ModelTemplateArtifact> {
        self.template_id
    }

    /// Returns the provider-visible model string supplied by the template.
    #[must_use]
    pub const fn wire_model(&self) -> &ModelName {
        &self.wire_model
    }

    /// Returns the selected user deployment.
    #[must_use]
    pub const fn deployment(&self) -> &DeploymentName {
        &self.deployment
    }

    /// Returns the endpoint owner label. It does not select a codec.
    #[must_use]
    pub const fn provider(&self) -> &ProviderName {
        &self.provider
    }

    /// Returns the protocol configuration selected from the model template.
    #[must_use]
    pub const fn protocol(&self) -> &ModelProtocolConfig {
        &self.protocol
    }

    /// Returns the user-configured deployment endpoint.
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

    /// Returns template defaults after bounded user overrides.
    #[must_use]
    pub const fn settings(&self) -> &ModelGenerationSettings {
        &self.settings
    }

    /// Returns model/protocol capabilities from the frozen template revision.
    #[must_use]
    pub const fn capabilities(&self) -> &ModelProtocolCapabilities {
        &self.capabilities
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

fn merge_settings(
    defaults: &ModelGenerationSettings,
    overrides: &ModelGenerationOverrides,
) -> ModelGenerationSettings {
    ModelGenerationSettings {
        max_output_tokens: overrides
            .max_output_tokens
            .unwrap_or(defaults.max_output_tokens),
        temperature_millis: overrides.temperature_millis.or(defaults.temperature_millis),
        reasoning: overrides
            .reasoning
            .clone()
            .unwrap_or_else(|| defaults.reasoning.clone()),
        parallel_tool_calls: overrides
            .parallel_tool_calls
            .unwrap_or(defaults.parallel_tool_calls),
    }
}

fn validate_protocol_config(
    owner: &str,
    protocol: &ModelProtocolConfig,
) -> Result<(), ModelCatalogError> {
    if matches!(
        protocol,
        ModelProtocolConfig::OpenAiResponses { store: true }
    ) {
        return Err(ModelCatalogError::HostedStateUnsupported(owner.to_owned()));
    }
    if let ModelProtocolConfig::AnthropicMessages { api_version } = protocol {
        if api_version.is_empty()
            || api_version.trim() != api_version
            || api_version.chars().any(char::is_control)
        {
            return Err(ModelCatalogError::InvalidApiVersion(owner.to_owned()));
        }
    }
    Ok(())
}

fn validate_capabilities(
    owner: &str,
    capabilities: &ModelProtocolCapabilities,
) -> Result<(), ModelCatalogError> {
    if capabilities.max_output_tokens.get() > capabilities.max_context_tokens.get() {
        return Err(ModelCatalogError::InvalidCapabilityBounds(owner.to_owned()));
    }
    if capabilities.supports_parallel_tool_calls && !capabilities.supports_tools {
        return Err(ModelCatalogError::ParallelToolsWithoutTools(
            owner.to_owned(),
        ));
    }
    if !capabilities.supports_reasoning && !capabilities.reasoning_efforts.is_empty() {
        return Err(ModelCatalogError::ReasoningEffortsWithoutReasoning(
            owner.to_owned(),
        ));
    }
    Ok(())
}

fn validate_generation(
    owner: &str,
    settings: &ModelGenerationSettings,
    capabilities: &ModelProtocolCapabilities,
) -> Result<(), ModelCatalogError> {
    if settings.max_output_tokens.get() > capabilities.max_output_tokens.get() {
        return Err(ModelCatalogError::OutputLimitExceeded(owner.to_owned()));
    }
    if settings.parallel_tool_calls && !capabilities.supports_parallel_tool_calls {
        return Err(ModelCatalogError::ParallelToolsUnsupported(
            owner.to_owned(),
        ));
    }
    if settings.parallel_tool_calls && !capabilities.supports_tools {
        return Err(ModelCatalogError::ToolsUnsupported(owner.to_owned()));
    }
    match (settings.reasoning.mode, settings.reasoning.effort) {
        (ModelReasoningMode::Disabled, Some(_)) => {
            return Err(ModelCatalogError::DisabledReasoningHasEffort(
                owner.to_owned(),
            ));
        }
        (ModelReasoningMode::Enabled, _) if !capabilities.supports_reasoning => {
            return Err(ModelCatalogError::ReasoningUnsupported(owner.to_owned()));
        }
        (ModelReasoningMode::Enabled, Some(effort))
            if !capabilities.reasoning_efforts.contains(&effort) =>
        {
            return Err(ModelCatalogError::ReasoningEffortUnsupported {
                owner: owner.to_owned(),
                effort,
            });
        }
        _ => {}
    }
    Ok(())
}

/// Invalid model template, runtime catalog, or resolution request.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ModelCatalogError {
    /// Runtime catalog schema is not supported by this build.
    #[error("unsupported runtime model catalog schema {0}")]
    UnsupportedRuntimeSchema(u16),
    /// One template file uses an unsupported schema.
    #[error("model template {template} uses unsupported schema {schema}")]
    UnsupportedTemplateSchema {
        /// Template name.
        template: String,
        /// Unsupported schema.
        schema: u16,
    },
    /// No templates were supplied to the registry.
    #[error("model template registry must not be empty")]
    EmptyTemplateRegistry,
    /// A template contains no protocol section.
    #[error("model template {0} contains no protocol definitions")]
    EmptyTemplateProtocols(String),
    /// Two files declared the same template name.
    #[error("duplicate model template {0}")]
    DuplicateTemplate(String),
    /// A template declared one protocol family more than once.
    #[error("model template {template} duplicates protocol {protocol:?}")]
    DuplicateTemplateProtocol {
        /// Template name.
        template: String,
        /// Repeated protocol.
        protocol: ModelProtocolKind,
    },
    /// Required runtime maps are empty.
    #[error("runtime model and deployment maps must not be empty")]
    EmptyRuntimeCatalog,
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
    /// Alias references an absent repository template.
    #[error("runtime model {alias} references unknown template {template}")]
    UnknownTemplate {
        /// Runtime-model alias.
        alias: String,
        /// Missing template name.
        template: String,
    },
    /// Configured HTTPS endpoint is unsafe or ambiguous.
    #[error("invalid provider HTTPS endpoint {0}")]
    InvalidEndpoint(String),
    /// Secret reference is empty, traversing, or otherwise ambiguous.
    #[error("invalid provider secret file path {0}")]
    InvalidSecretPath(String),
    /// V1 local-replay mode forbids hosted provider continuation.
    #[error("model template {0} enables unsupported hosted response state")]
    HostedStateUnsupported(String),
    /// Anthropic version header is not a stable label.
    #[error("model template {0} has an invalid Anthropic API version")]
    InvalidApiVersion(String),
    /// Capability output ceiling exceeds the context window.
    #[error("model template {0} has invalid token capability bounds")]
    InvalidCapabilityBounds(String),
    /// Parallel-tool capability was declared without tool capability.
    #[error("model template {0} declares parallel tools without tools")]
    ParallelToolsWithoutTools(String),
    /// Reasoning efforts were declared for a non-reasoning model/protocol.
    #[error("model template {0} declares reasoning efforts without reasoning")]
    ReasoningEffortsWithoutReasoning(String),
    /// Deployment protocol is outside the selected model template.
    #[error("runtime model {alias} template does not support {protocol:?}")]
    UnsupportedProtocol {
        /// Runtime-model alias.
        alias: String,
        /// Requested deployment protocol.
        protocol: ModelProtocolKind,
    },
    /// Resolved output setting exceeds template capability.
    #[error("model selection {0} output limit exceeds its template")]
    OutputLimitExceeded(String),
    /// Resolved settings request parallel tools without that capability.
    #[error("model selection {0} requests unsupported parallel tool calls")]
    ParallelToolsUnsupported(String),
    /// Resolved settings request tools from a model-only template.
    #[error("model selection {0} requests unsupported tools")]
    ToolsUnsupported(String),
    /// Resolved settings request reasoning without that capability.
    #[error("model selection {0} requests unsupported reasoning")]
    ReasoningUnsupported(String),
    /// Selected reasoning effort is outside the template set.
    #[error("model selection {owner} requests unsupported reasoning effort {effort:?}")]
    ReasoningEffortUnsupported {
        /// Template or alias under validation.
        owner: String,
        /// Unsupported effort.
        effort: ModelReasoningEffort,
    },
    /// Disabled reasoning cannot carry an effort value.
    #[error("model selection {0} configures effort while reasoning is disabled")]
    DisabledReasoningHasEffort(String),
    /// Template or frozen snapshot could not be encoded or identified.
    #[error("model configuration encoding failed: {0}")]
    Encoding(String),
}

#[cfg(test)]
mod tests {
    use super::{
        CredentialSource, ModelCatalogError, ModelOutputTokenLimit, ModelProtocolConfig,
        ModelProtocolKind, ModelTemplate, ModelTemplateRegistry, ProviderConfigValueError,
        ProviderEndpoint, RuntimeModelCatalog, SamplingTemperatureMillis, SecretFilePath,
    };
    use crate::{ModelName, RuntimeModelAlias};

    fn decode_fixture<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> T {
        let bytes = cairn_codec::canonicalize(bytes).expect("canonicalized fixture");
        cairn_codec::from_slice(&bytes).expect("typed fixture")
    }

    fn template() -> ModelTemplate {
        decode_fixture(include_bytes!(
            "../../../model-templates/deepseek/deepseek-v4-pro.json"
        ))
    }

    fn template_value() -> serde_json::Value {
        decode_fixture(include_bytes!(
            "../../../model-templates/deepseek/deepseek-v4-pro.json"
        ))
    }

    fn registry() -> ModelTemplateRegistry {
        ModelTemplateRegistry::from_templates([template()]).expect("template registry")
    }

    fn catalog() -> RuntimeModelCatalog {
        decode_fixture(include_bytes!(
            "../../../config/runtime-models.example.json"
        ))
    }

    fn catalog_value() -> serde_json::Value {
        decode_fixture(include_bytes!(
            "../../../config/runtime-models.example.json"
        ))
    }

    #[test]
    fn deepseek_responses_default_resolves_from_template_and_user_deployment() {
        let template = template();
        let expected_template_id = template.content_id().expect("template ID");
        let registry =
            ModelTemplateRegistry::from_templates([template]).expect("template registry");
        let catalog = catalog();
        catalog.validate(&registry).expect("valid catalog");
        let resolved = catalog.resolve(&registry, None).expect("default model");
        assert_eq!(
            resolved.wire_model(),
            &ModelName::new("deepseek-v4-pro").unwrap()
        );
        assert_eq!(resolved.template_id(), expected_template_id);
        assert_eq!(
            resolved.protocol().kind(),
            ModelProtocolKind::OpenAiResponses
        );
        assert!(resolved.capabilities().supports_tools());
        assert!(matches!(
            resolved.credential(),
            CredentialSource::BearerFile { .. }
        ));
        let bytes = resolved.canonical_bytes().expect("snapshot bytes");
        let text = std::str::from_utf8(&bytes).expect("UTF-8");
        assert!(!text.contains("sk-"));
        assert_eq!(
            resolved.content_id().unwrap(),
            resolved.content_id().unwrap()
        );
    }

    #[test]
    fn user_catalog_contains_no_model_capability_declarations() {
        let bytes = include_bytes!("../../../config/runtime-models.example.json");
        let text = std::str::from_utf8(bytes).expect("UTF-8 config");
        for forbidden in [
            "supports_tools",
            "supports_parallel_tool_calls",
            "supports_reasoning",
            "max_context_tokens",
            "tool_schema_dialect",
            "wire_model",
        ] {
            assert!(
                !text.contains(forbidden),
                "user config contains {forbidden}"
            );
        }
        let template_text = std::str::from_utf8(include_bytes!(
            "../../../model-templates/deepseek/deepseek-v4-pro.json"
        ))
        .expect("UTF-8 template");
        assert!(template_text.contains("supports_tools"));
        assert!(template_text.contains("wire_model"));
    }

    #[test]
    fn deepseek_template_resolves_all_three_protocol_families() {
        let mut value = catalog_value();
        let base_deployment = value["deployments"]["deepseek-responses"].clone();
        let base_runtime = value["runtime_models"]["deepseek-v4-pro"].clone();
        for (suffix, protocol, endpoint, credential) in [
            (
                "chat",
                serde_json::json!("openai_chat_completions"),
                serde_json::json!("https://private.example.test/v1/chat/completions"),
                serde_json::json!({"kind":"bearer_file","path":".cairn/secrets/deepseek-api-key"}),
            ),
            (
                "anthropic",
                serde_json::json!("anthropic_messages"),
                serde_json::json!("https://private.example.test/anthropic/v1/messages"),
                serde_json::json!({"kind":"x_api_key_file","path":".cairn/secrets/deepseek-api-key"}),
            ),
        ] {
            let deployment = format!("deepseek-{suffix}");
            let alias = format!("deepseek-v4-pro-{suffix}");
            value["deployments"][&deployment] = base_deployment.clone();
            value["deployments"][&deployment]["protocol"] = protocol;
            value["deployments"][&deployment]["endpoint"] = endpoint;
            value["deployments"][&deployment]["credential"] = credential;
            value["deployments"][&deployment]["data_boundary"] =
                serde_json::json!("private_deployment");
            value["runtime_models"][&alias] = base_runtime.clone();
            value["runtime_models"][&alias]["deployment"] = serde_json::json!(deployment);
        }
        let bytes = cairn_codec::to_vec(&value).expect("canonical catalog");
        let catalog: RuntimeModelCatalog = cairn_codec::from_slice(&bytes).expect("typed catalog");
        let registry = registry();
        let cases = [
            ("deepseek-v4-pro", ModelProtocolKind::OpenAiResponses),
            (
                "deepseek-v4-pro-chat",
                ModelProtocolKind::OpenAiChatCompletions,
            ),
            (
                "deepseek-v4-pro-anthropic",
                ModelProtocolKind::AnthropicMessages,
            ),
        ];
        for (alias, protocol) in cases {
            let alias = RuntimeModelAlias::new(alias).expect("alias");
            let resolved = catalog
                .resolve(&registry, Some(&alias))
                .expect("resolved protocol");
            assert_eq!(resolved.protocol().kind(), protocol);
            assert_eq!(resolved.wire_model().as_str(), "deepseek-v4-pro");
        }
    }

    #[test]
    fn private_endpoint_changes_deployment_not_model_characteristics() {
        let mut value = catalog_value();
        value["deployments"]["deepseek-responses"]["endpoint"] =
            serde_json::json!("https://llm.internal.example/v1/responses");
        value["deployments"]["deepseek-responses"]["provider"] =
            serde_json::json!("internal-platform");
        value["deployments"]["deepseek-responses"]["data_boundary"] =
            serde_json::json!("private_deployment");
        let bytes = cairn_codec::to_vec(&value).expect("canonical catalog");
        let catalog: RuntimeModelCatalog = cairn_codec::from_slice(&bytes).expect("typed catalog");
        let resolved = catalog.resolve(&registry(), None).expect("private model");
        assert_eq!(
            resolved.endpoint().as_str(),
            "https://llm.internal.example/v1/responses"
        );
        assert!(resolved.capabilities().supports_tools());
    }

    #[test]
    fn invalid_template_state_and_inline_secrets_are_rejected() {
        let mut value = template_value();
        value["protocols"][0]["protocol"] =
            serde_json::json!({"kind":"openai_responses","store":true});
        let bytes = cairn_codec::to_vec(&value).expect("canonical template");
        let template: ModelTemplate = cairn_codec::from_slice(&bytes).expect("typed template");
        assert!(matches!(
            template.validate(),
            Err(ModelCatalogError::HostedStateUnsupported(_))
        ));

        let mut value = catalog_value();
        value["deployments"]["deepseek-responses"]["credential"] =
            serde_json::json!({"kind":"bearer_file","api_key":"sk-secret"});
        let bytes = cairn_codec::to_vec(&value).expect("canonical invalid catalog");
        assert!(cairn_codec::from_slice::<RuntimeModelCatalog>(&bytes).is_err());
    }

    #[test]
    fn template_protocol_uniqueness_and_runtime_bounds_fail_closed() {
        let mut value = template_value();
        let duplicate = value["protocols"][0].clone();
        value["protocols"].as_array_mut().unwrap().push(duplicate);
        let bytes = cairn_codec::to_vec(&value).expect("canonical duplicate template");
        let template: ModelTemplate = cairn_codec::from_slice(&bytes).expect("typed template");
        assert!(matches!(
            template.validate(),
            Err(ModelCatalogError::DuplicateTemplateProtocol { .. })
        ));

        let mut value = catalog_value();
        value["runtime_models"]["deepseek-v4-pro"]["settings"] =
            serde_json::json!({"max_output_tokens":400_000});
        let bytes = cairn_codec::to_vec(&value).expect("canonical output override");
        let catalog: RuntimeModelCatalog = cairn_codec::from_slice(&bytes).expect("typed catalog");
        assert!(matches!(
            catalog.validate(&registry()),
            Err(ModelCatalogError::OutputLimitExceeded(_))
        ));

        let mut value = catalog_value();
        value["runtime_models"]["deepseek-v4-pro"]["settings"] = serde_json::json!({
            "reasoning":{"mode":"enabled","effort":"medium"}
        });
        let bytes = cairn_codec::to_vec(&value).expect("canonical effort override");
        let catalog: RuntimeModelCatalog = cairn_codec::from_slice(&bytes).expect("typed catalog");
        let mut value = template_value();
        value["protocols"][0]["capabilities"]["reasoning_efforts"] = serde_json::json!(["high"]);
        let bytes = cairn_codec::to_vec(&value).expect("canonical restricted template");
        let template: ModelTemplate = cairn_codec::from_slice(&bytes).expect("typed template");
        let templates = ModelTemplateRegistry::from_templates([template]).expect("registry");
        assert!(matches!(
            catalog.validate(&templates),
            Err(ModelCatalogError::ReasoningEffortUnsupported { .. })
        ));
    }

    #[test]
    fn deployment_cannot_select_a_protocol_absent_from_its_template() {
        let mut template = template_value();
        template["protocols"].as_array_mut().unwrap().truncate(1);
        let bytes = cairn_codec::to_vec(&template).expect("canonical restricted template");
        let template: ModelTemplate = cairn_codec::from_slice(&bytes).expect("typed template");
        let templates = ModelTemplateRegistry::from_templates([template]).expect("registry");

        let mut catalog = catalog_value();
        catalog["deployments"]["deepseek-responses"]["protocol"] =
            serde_json::json!("anthropic_messages");
        let bytes = cairn_codec::to_vec(&catalog).expect("canonical catalog");
        let catalog: RuntimeModelCatalog = cairn_codec::from_slice(&bytes).expect("typed catalog");
        assert!(matches!(
            catalog.validate(&templates),
            Err(ModelCatalogError::UnsupportedProtocol { .. })
        ));
    }

    #[test]
    fn protocol_is_selected_by_user_config_not_provider_label() {
        let mut catalog = catalog();
        catalog
            .deployments
            .get_mut("deepseek-responses")
            .unwrap()
            .provider = crate::ProviderName::new("not-a-vendor-branch").unwrap();
        let resolved = catalog.resolve(&registry(), None).expect("resolved");
        assert!(matches!(
            resolved.protocol(),
            ModelProtocolConfig::OpenAiResponses { .. }
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

        let mut value = catalog_value();
        value["runtime_models"][" invalid-alias"] =
            value["runtime_models"]["deepseek-v4-pro"].clone();
        let bytes = cairn_codec::to_vec(&value).expect("canonical invalid catalog");
        assert!(cairn_codec::from_slice::<RuntimeModelCatalog>(&bytes).is_err());
    }
}
