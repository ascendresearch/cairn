//! Process-level structured logging initialization for Cairn binaries and live gates.
//!
//! Durable domain events remain reconstruction and decision authority. Logs are operational
//! projections only and must never be read back to drive a state transition.

use std::{env, io};

use thiserror::Error;
use tracing_subscriber::EnvFilter;

const FILTER_ENV: &str = "CAIRN_LOG";
const FORMAT_ENV: &str = "CAIRN_LOG_FORMAT";
const DEFAULT_FILTER: &str = "info";

/// Supported stderr log encodings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LogFormat {
    Json,
    Compact,
}

impl LogFormat {
    fn parse(value: Option<&str>) -> Result<Self, ObservabilityError> {
        match value.unwrap_or("json") {
            "json" => Ok(Self::Json),
            "compact" => Ok(Self::Compact),
            value => Err(ObservabilityError::InvalidFormat(value.to_owned())),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Compact => "compact",
        }
    }
}

/// Logging configuration or global-subscriber initialization failure.
#[derive(Debug, Error)]
pub enum ObservabilityError {
    /// An environment value cannot be represented as UTF-8 configuration text.
    #[error("{0} must contain valid UTF-8")]
    InvalidEnvironmentEncoding(&'static str),
    /// The requested output encoding is not supported.
    #[error("{FORMAT_ENV} must be json or compact, received {0:?}")]
    InvalidFormat(String),
    /// The filter expression cannot be parsed.
    #[error("invalid {FILTER_ENV} filter: {0}")]
    InvalidFilter(String),
    /// A subscriber was already installed or could not be installed.
    #[error("logging subscriber initialization failed: {0}")]
    Subscriber(String),
    /// The hard-coded process component label is invalid.
    #[error("logging component must be a nonempty printable label of at most 64 characters")]
    InvalidComponent,
}

/// Installs the process-wide stderr subscriber.
///
/// `CAIRN_LOG` accepts `tracing_subscriber::EnvFilter` directives and defaults to `info`.
/// `CAIRN_LOG_FORMAT` is `json` by default and also accepts `compact`. ANSI output is disabled so
/// logs remain stable under service managers and collectors. Request/response bodies, prompts,
/// credentials, tool arguments/results, and workload stdout/stderr are never added here.
///
/// # Errors
///
/// Returns an error for an invalid component/filter/format or duplicate subscriber initialization.
pub fn init(component: &'static str) -> Result<(), ObservabilityError> {
    if component.is_empty() || component.len() > 64 || component.chars().any(char::is_control) {
        return Err(ObservabilityError::InvalidComponent);
    }
    let format_text = read_optional_environment(FORMAT_ENV)?;
    let format = LogFormat::parse(format_text.as_deref())?;
    let filter_text =
        read_optional_environment(FILTER_ENV)?.unwrap_or_else(|| DEFAULT_FILTER.to_owned());
    let filter = parse_filter(filter_text)?;
    match format {
        LogFormat::Json => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(io::stderr)
            .with_ansi(false)
            .with_target(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .json()
            .try_init()
            .map_err(|error| ObservabilityError::Subscriber(error.to_string()))?,
        LogFormat::Compact => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(io::stderr)
            .with_ansi(false)
            .with_target(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .compact()
            .try_init()
            .map_err(|error| ObservabilityError::Subscriber(error.to_string()))?,
    }
    tracing::info!(
        target: "cairn.observability",
        event = "logging_initialized",
        component,
        log_format = format.as_str(),
        filter_environment = FILTER_ENV,
        "structured logging initialized"
    );
    Ok(())
}

fn read_optional_environment(name: &'static str) -> Result<Option<String>, ObservabilityError> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => {
            Err(ObservabilityError::InvalidEnvironmentEncoding(name))
        }
    }
}

fn parse_filter(value: String) -> Result<EnvFilter, ObservabilityError> {
    EnvFilter::try_new(value).map_err(|error| ObservabilityError::InvalidFilter(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_is_strict_and_defaults_to_json() {
        assert_eq!(LogFormat::parse(None).expect("default"), LogFormat::Json);
        assert_eq!(
            LogFormat::parse(Some("compact")).expect("compact"),
            LogFormat::Compact
        );
        assert!(matches!(
            LogFormat::parse(Some("pretty")),
            Err(ObservabilityError::InvalidFormat(_))
        ));
    }

    #[test]
    fn filter_is_strict() {
        assert!(parse_filter("info,cairn.agent.model=debug".to_owned()).is_ok());
        assert!(matches!(
            parse_filter("[invalid".to_owned()),
            Err(ObservabilityError::InvalidFilter(_))
        ));
    }
}
