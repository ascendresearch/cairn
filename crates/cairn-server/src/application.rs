use std::future::Future;

use thiserror::Error;

/// Validated identity of an application composed above the domain-neutral server host.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ApplicationName(String);

impl ApplicationName {
    /// Creates a non-empty, trimmed process label.
    ///
    /// # Errors
    ///
    /// Rejects whitespace/control characters and empty names.
    pub fn new(value: impl Into<String>) -> Result<Self, ApplicationNameError> {
        let value = value.into();
        if value.is_empty()
            || value.trim() != value
            || value.chars().any(char::is_whitespace)
            || value.chars().any(char::is_control)
        {
            return Err(ApplicationNameError);
        }
        Ok(Self(value))
    }

    /// Returns the validated process label.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Invalid application identity.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("application name must be non-empty and contain no whitespace or control characters")]
pub struct ApplicationNameError;

/// Product composition started and supervised by `cairn-server` without teaching the server its
/// workflow, artifacts, roles, or admission rules.
pub trait ApplicationModule: Send + 'static {
    type Error: std::fmt::Display + Send + 'static;

    fn name(&self) -> &ApplicationName;

    fn run(self) -> impl Future<Output = Result<(), Self::Error>> + Send;
}
