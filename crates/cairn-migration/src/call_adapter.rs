//! Language-neutral isolated-process input protocol for operator call adapters.

use cairn_execution::{
    CommandArgument, CommandContract, InputBundleArtifact, InputBundleEntry, InputBundleV1,
    InputFileMode, SandboxPath,
};
use cairn_protocol::{ContentId, ContentType};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

use crate::{
    AssembledBoundaryCaseInput, AssembledInputValueCaseInput, AssembledMemorySurfaceCaseInput,
    MaterializedBoundaryCaseArtifact, MaterializedInputValueCaseArtifact,
    MaterializedMemorySurfaceCaseArtifact,
};

const ADAPTER_DIRECTORY: &str = "cairn/bin";
const ADAPTER_PATH: &str = "cairn/bin/call-adapter";
const REQUEST_PATH: &str = "cairn/call-adapter-request.json";
const INVOCATION_PATH: &str = "cairn/invocation.json";
const RESULT_PATH: &str = "cairn/call-adapter-result.json";
const CONTAINER_REQUEST_PATH: &str = "/cairn/input/cairn/call-adapter-request.json";
const CONTAINER_OUTPUT_ROOT: &str = "/cairn/output";
const WORKING_DIRECTORY: &str = "work";

/// Failure to bind a case bundle to one bounded isolated adapter executable.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CallAdapterProtocolError {
    /// Only the current pre-release V1 request is accepted.
    #[error("call-adapter request schema version must be 1")]
    UnsupportedSchemaVersion,
    /// An executable must contain at least one byte.
    #[error("call-adapter executable is empty")]
    EmptyExecutable,
    /// Executable bytes exceed the caller-supplied bound.
    #[error("call-adapter executable exceeds its byte limit")]
    ExecutableLimitExceeded,
    /// The supplied source bundle bytes do not match their typed identity.
    #[error("source case input bundle identity mismatch")]
    SourceBundleMismatch,
    /// Persisted request fields contradict the fixed process protocol.
    #[error("call-adapter request is inconsistent")]
    InconsistentRequest,
    /// Canonical encoding, path, command, or bundle construction failed.
    #[error("call-adapter protocol codec error: {message}")]
    Codec { message: String },
}

/// Positive maximum size accepted for one adapter executable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallAdapterExecutableByteLimit(u64);

impl CallAdapterExecutableByteLimit {
    /// Creates a positive executable bound.
    ///
    /// # Errors
    ///
    /// Rejects zero, which could never admit an executable.
    pub const fn new(value: u64) -> Result<Self, CallAdapterProtocolError> {
        if value == 0 {
            Err(CallAdapterProtocolError::ExecutableLimitExceeded)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the exact byte bound.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Content domain for exact executable bytes of an operator-specific call adapter.
pub enum CallAdapterExecutableArtifact {}

impl ContentType for CallAdapterExecutableArtifact {
    const DOMAIN: &'static str = "migration.call-adapter-executable.v1";
}

/// Strong identity of the exact case manifest the adapter must execute.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CorpusInvocationIdentityV1 {
    /// Quantitative boundary invocation.
    Boundary {
        manifest: ContentId<MaterializedBoundaryCaseArtifact>,
    },
    /// Supported or invalid dtype invocation.
    InputValue {
        manifest: ContentId<MaterializedInputValueCaseArtifact>,
    },
    /// Pointer, capacity, or aliasing invocation.
    MemorySurface {
        manifest: ContentId<MaterializedMemorySurfaceCaseArtifact>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CallAdapterSchemaV1;

impl Serialize for CallAdapterSchemaV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(1)
    }
}

impl<'de> Deserialize<'de> for CallAdapterSchemaV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match u32::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(de::Error::custom(
                CallAdapterProtocolError::UnsupportedSchemaVersion,
            )),
        }
    }
}

/// Strict V1 request read by a CUDA, Ascend C, or other isolated adapter process.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "CallAdapterRequestWire")]
pub struct CallAdapterRequestV1 {
    schema_version: CallAdapterSchemaV1,
    source_input_bundle: ContentId<InputBundleArtifact>,
    invocation: CorpusInvocationIdentityV1,
    executable: ContentId<CallAdapterExecutableArtifact>,
    invocation_path: SandboxPath,
    result_path: SandboxPath,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallAdapterRequestWire {
    schema_version: CallAdapterSchemaV1,
    source_input_bundle: ContentId<InputBundleArtifact>,
    invocation: CorpusInvocationIdentityV1,
    executable: ContentId<CallAdapterExecutableArtifact>,
    invocation_path: SandboxPath,
    result_path: SandboxPath,
}

impl CallAdapterRequestV1 {
    fn new(
        source_input_bundle: ContentId<InputBundleArtifact>,
        invocation: CorpusInvocationIdentityV1,
        executable: ContentId<CallAdapterExecutableArtifact>,
    ) -> Result<Self, CallAdapterProtocolError> {
        Ok(Self {
            schema_version: CallAdapterSchemaV1,
            source_input_bundle,
            invocation,
            executable,
            invocation_path: path(INVOCATION_PATH)?,
            result_path: path(RESULT_PATH)?,
        })
    }

    /// Returns the unmodified case bundle from which this process input was composed.
    #[must_use]
    pub const fn source_input_bundle(&self) -> ContentId<InputBundleArtifact> {
        self.source_input_bundle
    }

    /// Returns the exact typed invocation manifest identity.
    #[must_use]
    pub const fn invocation(&self) -> CorpusInvocationIdentityV1 {
        self.invocation
    }

    /// Returns the executable-byte identity selected for this invocation.
    #[must_use]
    pub const fn executable(&self) -> ContentId<CallAdapterExecutableArtifact> {
        self.executable
    }

    /// Returns the input-root-relative invocation manifest path.
    #[must_use]
    pub const fn invocation_path(&self) -> &SandboxPath {
        &self.invocation_path
    }

    /// Returns the output-root-relative result manifest path.
    #[must_use]
    pub const fn result_path(&self) -> &SandboxPath {
        &self.result_path
    }
}

impl TryFrom<CallAdapterRequestWire> for CallAdapterRequestV1 {
    type Error = CallAdapterProtocolError;

    fn try_from(wire: CallAdapterRequestWire) -> Result<Self, Self::Error> {
        let _ = wire.schema_version;
        let request = Self::new(wire.source_input_bundle, wire.invocation, wire.executable)?;
        if request.invocation_path != wire.invocation_path
            || request.result_path != wire.result_path
        {
            return Err(CallAdapterProtocolError::InconsistentRequest);
        }
        Ok(request)
    }
}

/// Content identity domain for an exact isolated call-adapter request.
pub enum CallAdapterRequestArtifact {}

impl ContentType for CallAdapterRequestArtifact {
    const DOMAIN: &'static str = "migration.call-adapter-request.v1";
}

/// Complete process input ready for execution-job composition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCallAdapterInput {
    request: CallAdapterRequestV1,
    request_id: ContentId<CallAdapterRequestArtifact>,
    input_bundle: InputBundleV1,
    input_bundle_bytes: Vec<u8>,
    input_bundle_id: ContentId<InputBundleArtifact>,
    command: CommandContract,
}

impl PreparedCallAdapterInput {
    #[must_use]
    pub const fn request(&self) -> &CallAdapterRequestV1 {
        &self.request
    }

    #[must_use]
    pub const fn request_id(&self) -> ContentId<CallAdapterRequestArtifact> {
        self.request_id
    }

    #[must_use]
    pub const fn input_bundle(&self) -> &InputBundleV1 {
        &self.input_bundle
    }

    #[must_use]
    pub fn input_bundle_bytes(&self) -> &[u8] {
        &self.input_bundle_bytes
    }

    #[must_use]
    pub const fn input_bundle_id(&self) -> ContentId<InputBundleArtifact> {
        self.input_bundle_id
    }

    #[must_use]
    pub const fn command(&self) -> &CommandContract {
        &self.command
    }
}

/// Binds a quantitative boundary case to one exact isolated adapter executable.
///
/// # Errors
///
/// Rejects empty/oversized executable bytes, a contradictory source bundle, or canonical protocol
/// construction failure.
pub fn prepare_boundary_call_adapter_input(
    case: &AssembledBoundaryCaseInput,
    executable: &[u8],
    limit: CallAdapterExecutableByteLimit,
) -> Result<PreparedCallAdapterInput, CallAdapterProtocolError> {
    prepare(
        case.input_bundle(),
        case.input_bundle_bytes(),
        case.input_bundle_id(),
        CorpusInvocationIdentityV1::Boundary {
            manifest: case.manifest_id(),
        },
        executable,
        limit,
    )
}

/// Binds a dtype case to one exact isolated adapter executable.
///
/// # Errors
///
/// Rejects empty/oversized executable bytes, a contradictory source bundle, or canonical protocol
/// construction failure.
pub fn prepare_input_value_call_adapter_input(
    case: &AssembledInputValueCaseInput,
    executable: &[u8],
    limit: CallAdapterExecutableByteLimit,
) -> Result<PreparedCallAdapterInput, CallAdapterProtocolError> {
    prepare(
        case.input_bundle(),
        case.input_bundle_bytes(),
        case.input_bundle_id(),
        CorpusInvocationIdentityV1::InputValue {
            manifest: case.manifest_id(),
        },
        executable,
        limit,
    )
}

/// Binds a memory-surface case to one exact isolated adapter executable.
///
/// # Errors
///
/// Rejects empty/oversized executable bytes, a contradictory source bundle, or canonical protocol
/// construction failure.
pub fn prepare_memory_surface_call_adapter_input(
    case: &AssembledMemorySurfaceCaseInput,
    executable: &[u8],
    limit: CallAdapterExecutableByteLimit,
) -> Result<PreparedCallAdapterInput, CallAdapterProtocolError> {
    prepare(
        case.input_bundle(),
        case.input_bundle_bytes(),
        case.input_bundle_id(),
        CorpusInvocationIdentityV1::MemorySurface {
            manifest: case.manifest_id(),
        },
        executable,
        limit,
    )
}

fn prepare(
    source: &InputBundleV1,
    source_bytes: &[u8],
    source_id: ContentId<InputBundleArtifact>,
    invocation: CorpusInvocationIdentityV1,
    executable: &[u8],
    limit: CallAdapterExecutableByteLimit,
) -> Result<PreparedCallAdapterInput, CallAdapterProtocolError> {
    if executable.is_empty() {
        return Err(CallAdapterProtocolError::EmptyExecutable);
    }
    if u64::try_from(executable.len()).map_or(true, |length| length > limit.get()) {
        return Err(CallAdapterProtocolError::ExecutableLimitExceeded);
    }
    if source.to_bytes().map_err(codec)? != source_bytes
        || ContentId::<InputBundleArtifact>::derive(source_bytes).map_err(codec)? != source_id
    {
        return Err(CallAdapterProtocolError::SourceBundleMismatch);
    }
    let executable_id =
        ContentId::<CallAdapterExecutableArtifact>::derive(executable).map_err(codec)?;
    let request = CallAdapterRequestV1::new(source_id, invocation, executable_id)?;
    let request_bytes = cairn_codec::to_vec(&request).map_err(codec)?;
    let request_id =
        ContentId::<CallAdapterRequestArtifact>::derive(&request_bytes).map_err(codec)?;
    let mut entries = source.entries().to_vec();
    entries.extend([
        InputBundleEntry::Directory {
            path: path(ADAPTER_DIRECTORY)?,
        },
        InputBundleEntry::File {
            path: path(ADAPTER_PATH)?,
            mode: InputFileMode::Executable,
            bytes: executable.to_vec(),
        },
        InputBundleEntry::File {
            path: path(REQUEST_PATH)?,
            mode: InputFileMode::Data,
            bytes: request_bytes,
        },
    ]);
    let input_bundle = InputBundleV1::new(entries).map_err(codec)?;
    let input_bundle_bytes = input_bundle.to_bytes().map_err(codec)?;
    let input_bundle_id =
        ContentId::<InputBundleArtifact>::derive(&input_bundle_bytes).map_err(codec)?;
    let command = CommandContract::new(
        path(ADAPTER_PATH)?,
        vec![
            argument("--request")?,
            argument(CONTAINER_REQUEST_PATH)?,
            argument("--output-root")?,
            argument(CONTAINER_OUTPUT_ROOT)?,
        ],
        path(WORKING_DIRECTORY)?,
    );
    Ok(PreparedCallAdapterInput {
        request,
        request_id,
        input_bundle,
        input_bundle_bytes,
        input_bundle_id,
        command,
    })
}

fn path(value: &str) -> Result<SandboxPath, CallAdapterProtocolError> {
    SandboxPath::new(value).map_err(codec)
}

fn argument(value: &str) -> Result<CommandArgument, CallAdapterProtocolError> {
    CommandArgument::new(value).map_err(codec)
}

fn codec(error: impl std::fmt::Display) -> CallAdapterProtocolError {
    CallAdapterProtocolError::Codec {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use cairn_execution::{InputBundleEntry, InputFileMode};
    use cairn_protocol::{ContentId, ContentType};
    use serde_json::json;

    use super::{
        ADAPTER_PATH, CallAdapterExecutableArtifact, CallAdapterExecutableByteLimit,
        CallAdapterProtocolError, CallAdapterRequestArtifact, CallAdapterRequestV1,
        CorpusInvocationIdentityV1, INVOCATION_PATH, REQUEST_PATH, path, prepare,
    };
    use crate::MaterializedBoundaryCaseArtifact;

    fn id<T: ContentType>(bytes: &[u8]) -> ContentId<T> {
        ContentId::<T>::derive(bytes).expect("content identity")
    }

    fn source_bundle() -> (
        cairn_execution::InputBundleV1,
        Vec<u8>,
        ContentId<cairn_execution::InputBundleArtifact>,
    ) {
        let bundle = cairn_execution::InputBundleV1::new(vec![
            InputBundleEntry::Directory {
                path: path("cairn").expect("root"),
            },
            InputBundleEntry::Directory {
                path: path("cairn/abi").expect("abi"),
            },
            InputBundleEntry::File {
                path: path(INVOCATION_PATH).expect("invocation"),
                mode: InputFileMode::Data,
                bytes: b"invocation".to_vec(),
            },
        ])
        .expect("source bundle");
        let bytes = bundle.to_bytes().expect("source bytes");
        let identity = id(&bytes);
        (bundle, bytes, identity)
    }

    #[test]
    fn process_input_binds_executable_request_source_and_fixed_command() {
        let (source, source_bytes, source_id) = source_bundle();
        let executable = b"ELF-adapter";
        let invocation = CorpusInvocationIdentityV1::Boundary {
            manifest: id::<MaterializedBoundaryCaseArtifact>(b"boundary"),
        };
        let prepared = prepare(
            &source,
            &source_bytes,
            source_id,
            invocation,
            executable,
            CallAdapterExecutableByteLimit::new(64).expect("limit"),
        )
        .expect("prepare");

        assert_eq!(prepared.request().source_input_bundle(), source_id);
        assert_eq!(prepared.request().invocation(), invocation);
        assert_eq!(
            prepared.request().executable(),
            id::<CallAdapterExecutableArtifact>(executable)
        );
        assert_eq!(
            prepared.request_id(),
            id::<CallAdapterRequestArtifact>(
                &cairn_codec::to_vec(prepared.request()).expect("request bytes")
            )
        );
        assert_eq!(
            prepared.input_bundle_id(),
            id(prepared.input_bundle_bytes())
        );
        assert_eq!(prepared.command().program().as_str(), ADAPTER_PATH);
        assert_eq!(prepared.command().working_directory().as_str(), "work");
        assert_eq!(
            prepared
                .command()
                .arguments()
                .iter()
                .map(cairn_execution::CommandArgument::as_str)
                .collect::<Vec<_>>(),
            vec![
                "--request",
                "/cairn/input/cairn/call-adapter-request.json",
                "--output-root",
                "/cairn/output"
            ]
        );

        let executable_entry = prepared
            .input_bundle()
            .entries()
            .iter()
            .find(|entry| entry.path().as_str() == ADAPTER_PATH)
            .expect("adapter executable");
        assert!(matches!(
            executable_entry,
            InputBundleEntry::File {
                mode: InputFileMode::Executable,
                bytes,
                ..
            } if bytes == executable
        ));
        let request_bytes = prepared
            .input_bundle()
            .entries()
            .iter()
            .find_map(|entry| match entry {
                InputBundleEntry::File { path, bytes, .. } if path.as_str() == REQUEST_PATH => {
                    Some(bytes)
                }
                _ => None,
            })
            .expect("request file");
        assert_eq!(
            cairn_codec::from_slice::<CallAdapterRequestV1>(request_bytes).expect("request decode"),
            *prepared.request()
        );
    }

    #[test]
    fn executable_bounds_source_identity_and_persisted_request_fail_closed() {
        let (source, source_bytes, source_id) = source_bundle();
        let invocation = CorpusInvocationIdentityV1::Boundary {
            manifest: id::<MaterializedBoundaryCaseArtifact>(b"boundary"),
        };
        let limit = CallAdapterExecutableByteLimit::new(4).expect("limit");
        assert_eq!(
            prepare(&source, &source_bytes, source_id, invocation, b"", limit),
            Err(CallAdapterProtocolError::EmptyExecutable)
        );
        assert_eq!(
            prepare(
                &source,
                &source_bytes,
                source_id,
                invocation,
                b"12345",
                limit,
            ),
            Err(CallAdapterProtocolError::ExecutableLimitExceeded)
        );
        assert_eq!(
            prepare(
                &source,
                &source_bytes,
                id(b"wrong bundle"),
                invocation,
                b"ELF",
                limit,
            ),
            Err(CallAdapterProtocolError::SourceBundleMismatch)
        );

        let prepared =
            prepare(&source, &source_bytes, source_id, invocation, b"ELF", limit).expect("prepare");
        let value = serde_json::to_value(prepared.request()).expect("request json");
        let mut wrong_version = value.clone();
        wrong_version["schema_version"] = json!(2);
        assert!(serde_json::from_value::<CallAdapterRequestV1>(wrong_version).is_err());
        let mut wrong_path = value.clone();
        wrong_path["result_path"] = json!("legacy/result.json");
        assert!(serde_json::from_value::<CallAdapterRequestV1>(wrong_path).is_err());
        let mut unknown = value;
        unknown["fallback_python"] = json!(true);
        assert!(serde_json::from_value::<CallAdapterRequestV1>(unknown).is_err());
    }
}
