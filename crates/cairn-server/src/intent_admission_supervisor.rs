//! Controller-owned supervision of one exact independent Intent Admission operation.

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use cairn_admission::{
    IntentAdmissionExecutableArtifact, IntentAdmissionPublicOutcomeV1,
    IntentAdmissionRestrictedStoreArtifact, UserIntentDecisionArtifact,
};
use cairn_protocol::ContentId;
use serde::{Deserialize, Deserializer, Serialize, de};
use tokio::{io::AsyncReadExt, process::Command, time::timeout};

use crate::{ServerConfig, ServerError};

macro_rules! positive_admission_quantity {
    ($(#[$meta:meta])* $name:ident, $maximum:expr) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Creates a positive process-management quantity.
            ///
            /// # Errors
            ///
            /// Rejects zero and values outside the current-V1 bound.
            pub fn new(value: u64) -> Result<Self, ServerError> {
                if value == 0 || value > $maximum {
                    Err(ServerError::Configuration(concat!(stringify!($name), " is outside its positive current-V1 bound").into()))
                } else {
                    Ok(Self(value))
                }
            }

            #[must_use]
            pub const fn get(self) -> u64 { self.0 }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where D: Deserializer<'de>,
            {
                Self::new(u64::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

positive_admission_quantity!(
    /// Maximum wall-clock duration of one exact independent Admission operation.
    IntentAdmissionProcessTimeoutMillis,
    86_400_000
);
positive_admission_quantity!(
    /// Maximum canonical public-outcome bytes accepted from Admission stdout.
    IntentAdmissionStdoutByteLimit,
    2 * 1024 * 1024
);
positive_admission_quantity!(
    /// Maximum observational diagnostic bytes retained from Admission stderr.
    IntentAdmissionStderrByteLimit,
    2 * 1024 * 1024
);

/// Exact process and restricted-store policy for independent Intent Admission.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IntentAdmissionProcessConfigV1 {
    pub executable: PathBuf,
    pub restricted_content_database: PathBuf,
    pub restricted_content_directory: PathBuf,
    pub process_timeout_ms: IntentAdmissionProcessTimeoutMillis,
    pub stdout_byte_limit: IntentAdmissionStdoutByteLimit,
    pub stderr_byte_limit: IntentAdmissionStderrByteLimit,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct RestrictedStoreIdentityV1<'a> {
    schema_version: u16,
    content_database: &'a Path,
    content_directory: &'a Path,
}

impl IntentAdmissionProcessConfigV1 {
    pub(crate) fn validate(&self, server: &ServerConfig) -> Result<(), ServerError> {
        if !self.executable.is_absolute()
            || !self.restricted_content_database.is_absolute()
            || !self.restricted_content_directory.is_absolute()
            || !server.storage.content_database.is_absolute()
            || !server.storage.content_directory.is_absolute()
        {
            return Err(ServerError::Configuration(
                "Intent Admission executable and public/restricted stores must use absolute paths"
                    .into(),
            ));
        }
        if !self.executable.is_file() {
            return Err(ServerError::Configuration(
                "Intent Admission executable must name an existing regular file".into(),
            ));
        }
        if self.restricted_content_database == server.storage.content_database
            || self.restricted_content_directory == server.storage.content_directory
            || self.restricted_content_database == self.restricted_content_directory
        {
            return Err(ServerError::Configuration(
                "Intent Admission public and restricted stores must remain distinct".into(),
            ));
        }
        let _ = self.executable_identity()?;
        let _ = self.restricted_store_identity()?;
        Ok(())
    }

    pub fn resolve_paths(&mut self, base: &Path) {
        resolve(&mut self.executable, base);
        resolve(&mut self.restricted_content_database, base);
        resolve(&mut self.restricted_content_directory, base);
    }

    pub(crate) fn executable_identity(
        &self,
    ) -> Result<ContentId<IntentAdmissionExecutableArtifact>, ServerError> {
        let mut executable = fs::File::open(&self.executable)
            .map_err(|error| ServerError::Configuration(error.to_string()))?;
        let byte_len = executable
            .metadata()
            .map_err(|error| ServerError::Configuration(error.to_string()))?
            .len();
        ContentId::derive_reader(&mut executable, byte_len).map_err(supervisor_error)
    }

    pub(crate) fn restricted_store_identity(
        &self,
    ) -> Result<ContentId<IntentAdmissionRestrictedStoreArtifact>, ServerError> {
        let bytes = cairn_codec::to_vec(&RestrictedStoreIdentityV1 {
            schema_version: 1,
            content_database: &self.restricted_content_database,
            content_directory: &self.restricted_content_directory,
        })
        .map_err(supervisor_error)?;
        ContentId::derive(&bytes).map_err(supervisor_error)
    }
}

/// Independent Admission failure categories that require explicit reconciliation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntentAdmissionProcessBlockedV1 {
    InvocationDrift,
    TimedOut,
    ExitFailure,
    StdoutLimitExceeded,
    StderrLimitExceeded,
    InvalidOutcome,
}

pub(crate) struct IntentAdmissionProcessFailure {
    pub(crate) reason: IntentAdmissionProcessBlockedV1,
}

/// Runs the independently authorized gate and accepts only one canonical public outcome.
pub(crate) async fn run_intent_admission_process(
    config: &IntentAdmissionProcessConfigV1,
    server: &ServerConfig,
    decision: ContentId<UserIntentDecisionArtifact>,
    executable: ContentId<IntentAdmissionExecutableArtifact>,
    restricted_store: ContentId<IntentAdmissionRestrictedStoreArtifact>,
) -> Result<IntentAdmissionPublicOutcomeV1, IntentAdmissionProcessFailure> {
    validate_authorized_operation(config, server, executable, restricted_store)?;
    let mut child = Command::new(&config.executable)
        .arg("promote-user-intent")
        .arg(&server.storage.content_database)
        .arg(&server.storage.content_directory)
        .arg(&config.restricted_content_database)
        .arg(&config.restricted_content_directory)
        .arg(decision.to_string())
        .env_clear()
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| failure(IntentAdmissionProcessBlockedV1::ExitFailure, error))?;
    let stdout = child.stdout.take().ok_or_else(|| {
        failure_message(
            IntentAdmissionProcessBlockedV1::ExitFailure,
            "Intent Admission stdout pipe is absent",
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        failure_message(
            IntentAdmissionProcessBlockedV1::ExitFailure,
            "Intent Admission stderr pipe is absent",
        )
    })?;
    let stdout_limit = config.stdout_byte_limit.get();
    let stderr_limit = config.stderr_byte_limit.get();
    let stdout_reader = tokio::spawn(read_bounded(stdout, stdout_limit));
    let stderr_reader = tokio::spawn(read_bounded(stderr, stderr_limit));
    let status = if let Ok(result) = timeout(
        Duration::from_millis(config.process_timeout_ms.get()),
        child.wait(),
    )
    .await
    {
        result.map_err(|error| failure(IntentAdmissionProcessBlockedV1::ExitFailure, error))?
    } else {
        let _ = child.kill().await;
        let _ = child.wait().await;
        return Err(failure_message(
            IntentAdmissionProcessBlockedV1::TimedOut,
            "Intent Admission process exceeded its exact wall-clock limit",
        ));
    };
    let stdout = join_reader(stdout_reader).await?;
    let stderr = join_reader(stderr_reader).await?;
    if stdout.len() > usize::try_from(stdout_limit).unwrap_or(usize::MAX) {
        return Err(failure_message(
            IntentAdmissionProcessBlockedV1::StdoutLimitExceeded,
            "Intent Admission stdout exceeded its configured byte limit",
        ));
    }
    if stderr.len() > usize::try_from(stderr_limit).unwrap_or(usize::MAX) {
        return Err(failure_message(
            IntentAdmissionProcessBlockedV1::StderrLimitExceeded,
            "Intent Admission stderr exceeded its configured byte limit",
        ));
    }
    if !status.success() {
        return Err(IntentAdmissionProcessFailure {
            reason: IntentAdmissionProcessBlockedV1::ExitFailure,
        });
    }
    let outcome: IntentAdmissionPublicOutcomeV1 = cairn_codec::from_slice(&stdout)
        .map_err(|error| failure(IntentAdmissionProcessBlockedV1::InvalidOutcome, error))?;
    if cairn_codec::to_vec(&outcome).ok().as_deref() != Some(stdout.as_slice()) {
        return Err(failure_message(
            IntentAdmissionProcessBlockedV1::InvalidOutcome,
            "Intent Admission returned a noncanonical public outcome",
        ));
    }
    Ok(outcome)
}

async fn read_bounded<R: tokio::io::AsyncRead + Unpin>(
    reader: R,
    limit: u64,
) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes).await?;
    Ok(bytes)
}

async fn join_reader(
    handle: tokio::task::JoinHandle<std::io::Result<Vec<u8>>>,
) -> Result<Vec<u8>, IntentAdmissionProcessFailure> {
    handle
        .await
        .map_err(|error| failure(IntentAdmissionProcessBlockedV1::ExitFailure, error))?
        .map_err(|error| failure(IntentAdmissionProcessBlockedV1::ExitFailure, error))
}

fn validate_authorized_operation(
    config: &IntentAdmissionProcessConfigV1,
    server: &ServerConfig,
    executable: ContentId<IntentAdmissionExecutableArtifact>,
    restricted_store: ContentId<IntentAdmissionRestrictedStoreArtifact>,
) -> Result<(), IntentAdmissionProcessFailure> {
    config
        .validate(server)
        .map_err(|error| failure(IntentAdmissionProcessBlockedV1::InvocationDrift, error))?;
    if config
        .executable_identity()
        .map_err(|error| failure(IntentAdmissionProcessBlockedV1::InvocationDrift, error))?
        != executable
        || config
            .restricted_store_identity()
            .map_err(|error| failure(IntentAdmissionProcessBlockedV1::InvocationDrift, error))?
            != restricted_store
    {
        return Err(failure_message(
            IntentAdmissionProcessBlockedV1::InvocationDrift,
            "Intent Admission executable or restricted store changed after durable authority",
        ));
    }
    Ok(())
}

fn resolve(path: &mut PathBuf, base: &Path) {
    if path.is_relative() {
        *path = base.join(&*path);
    }
}

fn failure(
    reason: IntentAdmissionProcessBlockedV1,
    _error: impl std::fmt::Display,
) -> IntentAdmissionProcessFailure {
    IntentAdmissionProcessFailure { reason }
}

fn failure_message(
    reason: IntentAdmissionProcessBlockedV1,
    _diagnostic: &str,
) -> IntentAdmissionProcessFailure {
    IntentAdmissionProcessFailure { reason }
}

fn supervisor_error(error: impl std::fmt::Display) -> ServerError {
    ServerError::MigrationWorkflow(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use cairn_protocol::{ContentId, TaskId};
    use serde_json::{Value, json};

    use super::*;

    fn server(root: &Path) -> ServerConfig {
        let mut value: Value =
            serde_json::from_str(include_str!("../../../config/controller.example.json"))
                .expect("controller example");
        value["storage"]["event_database"] =
            json!(root.join("events.db").to_string_lossy().into_owned());
        value["storage"]["content_database"] =
            json!(root.join("public.db").to_string_lossy().into_owned());
        value["storage"]["content_directory"] =
            json!(root.join("public-cas").to_string_lossy().into_owned());
        serde_json::from_value(value).expect("server configuration")
    }

    fn config(root: &Path, executable: &str) -> IntentAdmissionProcessConfigV1 {
        IntentAdmissionProcessConfigV1 {
            executable: PathBuf::from(executable),
            restricted_content_database: root.join("restricted.db"),
            restricted_content_directory: root.join("restricted-cas"),
            process_timeout_ms: IntentAdmissionProcessTimeoutMillis::new(1_000).expect("timeout"),
            stdout_byte_limit: IntentAdmissionStdoutByteLimit::new(64 * 1024).expect("stdout"),
            stderr_byte_limit: IntentAdmissionStderrByteLimit::new(64 * 1024).expect("stderr"),
        }
    }

    #[tokio::test]
    async fn authorized_operation_rejects_store_drift_before_child_effect() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let server = server(temporary.path());
        let mut config = config(temporary.path(), "/bin/false");
        let executable = config.executable_identity().expect("executable identity");
        let restricted_store = config
            .restricted_store_identity()
            .expect("restricted store identity");
        config.restricted_content_directory = temporary.path().join("drifted-restricted-cas");

        let failure = run_intent_admission_process(
            &config,
            &server,
            ContentId::<UserIntentDecisionArtifact>::derive(TaskId::new().to_string().as_bytes())
                .expect("decision identity"),
            executable,
            restricted_store,
        )
        .await
        .expect_err("store drift must fail before spawning the authorized effect");
        assert_eq!(
            failure.reason,
            IntentAdmissionProcessBlockedV1::InvocationDrift
        );
    }

    #[tokio::test]
    async fn successful_child_exit_still_requires_a_canonical_typed_outcome() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let server = server(temporary.path());
        let config = config(temporary.path(), "/bin/echo");
        let failure = run_intent_admission_process(
            &config,
            &server,
            ContentId::<UserIntentDecisionArtifact>::derive(b"decision")
                .expect("decision identity"),
            config.executable_identity().expect("executable identity"),
            config
                .restricted_store_identity()
                .expect("restricted store identity"),
        )
        .await
        .expect_err("untyped stdout must not become an observation");
        assert_eq!(
            failure.reason,
            IntentAdmissionProcessBlockedV1::InvalidOutcome
        );
    }
}
