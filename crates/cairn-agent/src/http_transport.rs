//! Bounded one-turn HTTPS model transport with dispatch-time credential resolution.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

use reqwest::{
    blocking::Client,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue},
};
use thiserror::Error;

use crate::{
    CredentialSource, ModelProtocolKind, ModelTransport, ModelTransportResponse,
    PreparedModelRequest, ProviderCacheTokenUsage, ProviderTokenCount, ProviderTokenUsage,
    ResolvedRuntimeModel, TransportError,
};

const X_API_KEY: HeaderName = HeaderName::from_static("x-api-key");

/// Failure while constructing the reusable, secret-free HTTPS client.
#[derive(Debug, Error)]
pub enum HttpTransportConfigError {
    /// TLS/HTTP client configuration failed.
    #[error("HTTPS model client configuration failed: {0}")]
    Client(String),
}

/// Blocking one-exchange transport. It owns no agent-loop, retry, or tool behavior.
pub struct HttpModelTransport {
    client: Client,
    endpoint: String,
    credential: CredentialSource,
    credential_base: PathBuf,
    protocol: ModelProtocolKind,
    max_request_bytes: u64,
    max_response_bytes: u64,
}

impl HttpModelTransport {
    /// Creates a bounded client from one frozen runtime-model resolution without reading secrets.
    ///
    /// Relative credential references are resolved against `credential_base` at dispatch time.
    /// Redirects are disabled so credentials cannot be forwarded to another authority.
    ///
    /// # Errors
    ///
    /// Returns [`HttpTransportConfigError`] when the underlying HTTPS client cannot be built.
    pub fn new(
        model: &ResolvedRuntimeModel,
        credential_base: impl AsRef<Path>,
    ) -> Result<Self, HttpTransportConfigError> {
        let transport = model.transport();
        let mut builder = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("cairn/", env!("CARGO_PKG_VERSION")));
        if let Some(timeout) = transport.connect_timeout_ms {
            builder = builder.connect_timeout(Duration::from_millis(timeout.get()));
        }
        if let Some(timeout) = transport.request_timeout_ms {
            builder = builder.timeout(Duration::from_millis(timeout.get()));
        }
        let client = builder
            .build()
            .map_err(|error| HttpTransportConfigError::Client(error.to_string()))?;
        Ok(Self {
            client,
            endpoint: model.endpoint().as_str().to_owned(),
            credential: model.credential().clone(),
            credential_base: credential_base.as_ref().to_path_buf(),
            protocol: model.protocol().kind(),
            max_request_bytes: transport.max_request_bytes.get(),
            max_response_bytes: transport.max_response_bytes.get(),
        })
    }

    fn credential_path(&self) -> PathBuf {
        self.credential_base.join(self.credential.path().as_str())
    }

    fn authorization_headers(&self) -> Result<HeaderMap, TransportError> {
        let path = self.credential_path();
        let metadata = fs::metadata(&path).map_err(|error| {
            TransportError::NotSent(format!("credential file is unavailable: {error}"))
        })?;
        if !metadata.is_file() {
            return Err(TransportError::NotSent(
                "credential reference is not a regular file".to_owned(),
            ));
        }
        let secret = fs::read_to_string(path).map_err(|error| {
            TransportError::NotSent(format!("credential file cannot be read: {error}"))
        })?;
        let secret = secret.trim_end_matches(['\r', '\n']);
        if secret.is_empty()
            || secret.chars().any(char::is_whitespace)
            || secret.chars().any(char::is_control)
        {
            return Err(TransportError::NotSent(
                "credential file does not contain one non-empty token".to_owned(),
            ));
        }
        let wire_value = match self.credential {
            CredentialSource::BearerFile { .. } => format!("Bearer {secret}"),
            CredentialSource::XApiKeyFile { .. } => secret.to_owned(),
        };
        let mut value = HeaderValue::from_str(&wire_value).map_err(|_| {
            TransportError::NotSent("credential cannot be represented as an HTTP header".to_owned())
        })?;
        value.set_sensitive(true);
        let mut headers = HeaderMap::new();
        match self.credential {
            CredentialSource::BearerFile { .. } => {
                headers.insert(AUTHORIZATION, value);
            }
            CredentialSource::XApiKeyFile { .. } => {
                headers.insert(X_API_KEY, value);
            }
        }
        Ok(headers)
    }
}

impl ModelTransport for HttpModelTransport {
    fn dispatch(
        &mut self,
        request: &PreparedModelRequest,
    ) -> Result<ModelTransportResponse, TransportError> {
        let request_len = u64::try_from(request.request_bytes().len()).map_err(|_| {
            TransportError::NotSent("model request length cannot be represented".to_owned())
        })?;
        if request_len > self.max_request_bytes {
            return Err(TransportError::NotSent(format!(
                "model request has {request_len} bytes; configured maximum is {}",
                self.max_request_bytes
            )));
        }
        let headers = self.authorization_headers()?;
        let response = self
            .client
            .post(&self.endpoint)
            .headers(headers)
            .header(CONTENT_TYPE, "application/json")
            .body(request.request_bytes().to_vec())
            .send()
            .map_err(|error| classify_send_error(&error))?;
        let status = response.status();
        let declared_len = response.content_length();
        let maximum_plus_one = self.max_response_bytes.saturating_add(1);
        let mut bytes = Vec::new();
        response
            .take(maximum_plus_one)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                TransportError::Ambiguous(format!("provider response body failed: {error}"))
            })?;
        let response_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if response_len > self.max_response_bytes
            || declared_len.is_some_and(|length| length > self.max_response_bytes)
        {
            let message = format!(
                "provider response exceeds configured {} byte maximum",
                self.max_response_bytes
            );
            return if status.is_success() {
                Err(TransportError::Ambiguous(message))
            } else {
                Err(TransportError::Rejected(format!(
                    "HTTP {status}; {message}"
                )))
            };
        }
        if !status.is_success() {
            let detail = provider_error_summary(&bytes)
                .map(|message| format!("; {message}"))
                .unwrap_or_default();
            return Err(TransportError::Rejected(format!("HTTP {status}{detail}")));
        }
        let usage = response_usage(&bytes, self.protocol);
        Ok(ModelTransportResponse::new(bytes, usage))
    }
}

fn classify_send_error(error: &reqwest::Error) -> TransportError {
    if error.is_builder() || error.is_connect() {
        TransportError::NotSent(format!("HTTPS request could not be sent: {error}"))
    } else {
        TransportError::Ambiguous(format!("HTTPS request outcome is unknown: {error}"))
    }
}

fn provider_error_summary(bytes: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let message = value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)?;
    let sanitized = message
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(512)
        .collect::<String>();
    (!sanitized.is_empty()).then_some(sanitized)
}

fn response_usage(bytes: &[u8], protocol: ModelProtocolKind) -> Option<ProviderTokenUsage> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let usage = value.get("usage")?;
    match protocol {
        ModelProtocolKind::OpenAiResponses => openai_responses_usage(usage),
        ModelProtocolKind::OpenAiChatCompletions => openai_chat_usage(usage),
        ModelProtocolKind::AnthropicMessages => anthropic_usage(usage),
    }
}

fn openai_responses_usage(usage: &serde_json::Value) -> Option<ProviderTokenUsage> {
    let input = usage.get("input_tokens")?.as_u64()?;
    let output = usage.get("output_tokens")?.as_u64()?;
    let details = usage.get("input_tokens_details");
    usage_with_optional_cache(
        input,
        output,
        details
            .and_then(|value| value.get("cached_tokens"))
            .and_then(serde_json::Value::as_u64),
        details
            .and_then(|value| value.get("cache_write_tokens"))
            .and_then(serde_json::Value::as_u64),
        None,
    )
}

fn openai_chat_usage(usage: &serde_json::Value) -> Option<ProviderTokenUsage> {
    let input = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))?
        .as_u64()?;
    let output = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))?
        .as_u64()?;
    let details = usage
        .get("prompt_tokens_details")
        .or_else(|| usage.get("input_tokens_details"));
    usage_with_optional_cache(
        input,
        output,
        usage
            .get("prompt_cache_hit_tokens")
            .and_then(serde_json::Value::as_u64)
            .or_else(|| {
                details
                    .and_then(|value| value.get("cached_tokens"))
                    .and_then(serde_json::Value::as_u64)
            }),
        details
            .and_then(|value| value.get("cache_write_tokens"))
            .and_then(serde_json::Value::as_u64),
        usage
            .get("prompt_cache_miss_tokens")
            .and_then(serde_json::Value::as_u64),
    )
}

fn anthropic_usage(usage: &serde_json::Value) -> Option<ProviderTokenUsage> {
    let uncached = usage.get("input_tokens")?.as_u64()?;
    let output = usage.get("output_tokens")?.as_u64()?;
    let read = usage
        .get("cache_read_input_tokens")
        .and_then(serde_json::Value::as_u64);
    let write = usage
        .get("cache_creation_input_tokens")
        .and_then(serde_json::Value::as_u64);
    let input = uncached
        .checked_add(read.unwrap_or(0))?
        .checked_add(write.unwrap_or(0))?;
    usage_with_optional_cache(input, output, read, write, Some(uncached))
}

fn usage_with_optional_cache(
    input: u64,
    output: u64,
    read: Option<u64>,
    write: Option<u64>,
    miss: Option<u64>,
) -> Option<ProviderTokenUsage> {
    let input = ProviderTokenCount::new(input);
    let output = ProviderTokenCount::new(output);
    if read.is_none() && write.is_none() && miss.is_none() {
        return ProviderTokenUsage::new(input, output).ok();
    }
    let cache = ProviderCacheTokenUsage::new(
        read.map(ProviderTokenCount::new),
        write.map(ProviderTokenCount::new),
        miss.map(ProviderTokenCount::new),
    )
    .ok()?;
    ProviderTokenUsage::with_cache_tokens(input, output, cache).ok()
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Cursor};

    use cairn_protocol::{ContentId, ContentType};
    use cairn_record::ContentStore;
    use cairn_store_sqlite::SqliteContentStore;

    use super::{provider_error_summary, response_usage};
    use crate::{
        AdapterVersion, ContextBlock, DeploymentName, HistoryItem, HttpModelTransport,
        InstructionBlock, ModelName, ModelProtocolKind, ModelSelection, ModelTransport,
        OperationResult, PolicyDocument, ProviderName, ProviderTokenCount, ToolCatalog,
        TransportError, TurnInputDecision, prepare_model_request,
    };

    fn put_json<T: ContentType>(
        store: &mut SqliteContentStore,
        value: &serde_json::Value,
    ) -> ContentId<T> {
        let bytes = cairn_codec::to_vec(value).expect("fixture JSON");
        store
            .put::<T>(&mut Cursor::new(bytes))
            .expect("store fixture")
            .content_id
    }

    fn oversized_request(directory: &tempfile::TempDir) -> crate::PreparedModelRequest {
        let mut store = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("store");
        let decision = TurnInputDecision {
            selection: ModelSelection {
                provider: ProviderName::new("fixture").expect("provider"),
                model: ModelName::new("fixture").expect("model"),
                deployment: DeploymentName::new("fixture").expect("deployment"),
                adapter_version: AdapterVersion::new("fixture-v1").expect("adapter"),
            },
            instructions: vec![put_json::<InstructionBlock>(
                &mut store,
                &serde_json::json!({"text":"large request fixture"}),
            )],
            tool_catalog: put_json::<ToolCatalog>(&mut store, &serde_json::json!({"tools":[]})),
            history: vec![put_json::<HistoryItem>(
                &mut store,
                &serde_json::json!({"role":"user","content":"fixture"}),
            )],
            context: Vec::<ContentId<ContextBlock>>::new(),
            pending_results: Vec::<ContentId<OperationResult>>::new(),
            policy: put_json::<PolicyDocument>(&mut store, &serde_json::json!({"network":"allow"})),
        };
        prepare_model_request(&mut store, &decision).expect("request")
    }

    #[test]
    fn provider_error_summary_is_bounded_and_control_free() {
        let body = serde_json::json!({"error":{"message":format!("bad\n{}", "x".repeat(600))}});
        let summary =
            provider_error_summary(&serde_json::to_vec(&body).expect("JSON")).expect("summary");
        assert!(summary.len() <= 512);
        assert!(!summary.chars().any(char::is_control));
    }

    #[test]
    fn usage_is_accepted_only_when_both_counts_are_valid() {
        let usage = response_usage(
            br#"{"usage":{"input_tokens":7,"output_tokens":3}}"#,
            ModelProtocolKind::OpenAiResponses,
        )
        .expect("usage");
        assert_eq!(usage.input_tokens(), ProviderTokenCount::new(7));
        assert_eq!(usage.output_tokens(), ProviderTokenCount::new(3));
        assert!(
            response_usage(
                br#"{"usage":{"input_tokens":7}}"#,
                ModelProtocolKind::OpenAiResponses
            )
            .is_none()
        );
    }

    #[test]
    fn protocol_usage_retains_cache_observations_without_inference() {
        let responses = response_usage(
            br#"{"usage":{"input_tokens":11,"input_tokens_details":{"cached_tokens":8,"cache_write_tokens":2},"output_tokens":3}}"#,
            ModelProtocolKind::OpenAiResponses,
        )
        .expect("Responses usage");
        let cache = responses.cache_tokens().expect("Responses cache detail");
        assert_eq!(cache.read_tokens(), Some(ProviderTokenCount::new(8)));
        assert_eq!(cache.write_tokens(), Some(ProviderTokenCount::new(2)));
        assert_eq!(cache.miss_tokens(), None);

        let chat = response_usage(
            br#"{"usage":{"prompt_tokens":11,"completion_tokens":3,"prompt_cache_hit_tokens":8,"prompt_cache_miss_tokens":3}}"#,
            ModelProtocolKind::OpenAiChatCompletions,
        )
        .expect("Chat usage");
        let cache = chat.cache_tokens().expect("Chat cache detail");
        assert_eq!(cache.read_tokens(), Some(ProviderTokenCount::new(8)));
        assert_eq!(cache.write_tokens(), None);
        assert_eq!(cache.miss_tokens(), Some(ProviderTokenCount::new(3)));

        let anthropic = response_usage(
            br#"{"usage":{"input_tokens":3,"cache_read_input_tokens":8,"cache_creation_input_tokens":2,"output_tokens":3}}"#,
            ModelProtocolKind::AnthropicMessages,
        )
        .expect("Anthropic usage");
        assert_eq!(anthropic.input_tokens(), ProviderTokenCount::new(13));
        let cache = anthropic.cache_tokens().expect("Anthropic cache detail");
        assert_eq!(cache.read_tokens(), Some(ProviderTokenCount::new(8)));
        assert_eq!(cache.write_tokens(), Some(ProviderTokenCount::new(2)));
        assert_eq!(cache.miss_tokens(), Some(ProviderTokenCount::new(3)));

        let no_detail = response_usage(
            br#"{"usage":{"input_tokens":7,"output_tokens":3}}"#,
            ModelProtocolKind::OpenAiResponses,
        )
        .expect("usage without cache detail");
        assert_eq!(no_detail.cache_tokens(), None);
    }

    #[test]
    fn request_bound_fails_before_secret_or_network_access() {
        let directory = tempfile::tempdir().expect("tempdir");
        let request = oversized_request(&directory);
        let mut resolved: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../config/runtime-models.example.json"
        ))
        .expect("catalog JSON");
        resolved["deployments"]["deepseek-responses"]["transport"]["max_request_bytes"] =
            serde_json::json!(1);
        let catalog: crate::RuntimeModelCatalog =
            serde_json::from_value(resolved).expect("catalog");
        let template: crate::ModelTemplate = serde_json::from_slice(include_bytes!(
            "../../../model-templates/deepseek/deepseek-v4-pro.json"
        ))
        .expect("template");
        let registry = crate::ModelTemplateRegistry::from_templates([template]).expect("registry");
        let model = catalog.resolve(&registry, None).expect("resolved");
        fs::create_dir_all(directory.path().join(".cairn/secrets")).expect("secret directory");
        let mut transport = HttpModelTransport::new(&model, directory.path()).expect("transport");
        assert!(matches!(
            transport.dispatch(&request),
            Err(TransportError::NotSent(message)) if message.contains("configured maximum")
        ));
    }

    #[test]
    fn credential_diagnostic_never_contains_secret_bytes() {
        let directory = tempfile::tempdir().expect("tempdir");
        let secret_path = directory.path().join(".cairn/secrets");
        fs::create_dir_all(&secret_path).expect("secret directory");
        fs::write(secret_path.join("deepseek-api-key"), "secret with spaces")
            .expect("write secret");
        let catalog: crate::RuntimeModelCatalog = serde_json::from_slice(include_bytes!(
            "../../../config/runtime-models.example.json"
        ))
        .expect("catalog");
        let template: crate::ModelTemplate = serde_json::from_slice(include_bytes!(
            "../../../model-templates/deepseek/deepseek-v4-pro.json"
        ))
        .expect("template");
        let registry = crate::ModelTemplateRegistry::from_templates([template]).expect("registry");
        let model = catalog.resolve(&registry, None).expect("resolved");
        let transport = HttpModelTransport::new(&model, directory.path()).expect("transport");
        let diagnostic = transport
            .authorization_headers()
            .expect_err("invalid secret")
            .to_string();
        assert!(!diagnostic.contains("secret with spaces"));
    }
}
