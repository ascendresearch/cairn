//! Shared deterministic host implementation of the isolated call-adapter process protocol.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use cairn_migration::{
    CallAdapterCompletionV1, CallAdapterObservedOutputV1, CallAdapterRequestArtifact,
    CallAdapterRequestV1, CallAdapterResultV1, CorpusInvocationIdentityV1,
    ExecutableOracleInvocationArtifact, ExecutableOracleInvocationV1,
    MaterializedBoundaryCaseArtifact, MaterializedBoundaryCaseV1,
    MaterializedInputValueCaseArtifact, MaterializedInputValueCaseV1,
    MaterializedMemorySurfaceCaseArtifact, MaterializedMemorySurfaceCaseV1,
};
use cairn_protocol::{ContentId, ContentType};

pub fn main_with_output_byte(output_byte: u8) {
    if let Err(error) = run(std::env::args_os().skip(1), output_byte) {
        eprintln!("call-adapter fixture failed: {error}");
        std::process::exit(1);
    }
}

fn run(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
    output_byte: u8,
) -> Result<(), Box<dyn Error>> {
    let (request_path, output_root) = parse_arguments(arguments)?;
    let request_bytes = fs::read(&request_path)?;
    let request: CallAdapterRequestV1 = cairn_codec::from_slice(&request_bytes)?;
    validate_invocation(&request_path, &request)?;

    let mut observed = Vec::with_capacity(request.expected_outputs().len());
    for expected in request.expected_outputs() {
        let length = usize::try_from(expected.byte_length().get())?;
        let bytes = vec![output_byte; length];
        write_output(&output_root, expected.path().as_str(), &bytes)?;
        observed.push(CallAdapterObservedOutputV1::from_bytes(
            expected.argument_index(),
            expected.buffer().clone(),
            &bytes,
        )?);
    }
    let request_id = ContentId::<CallAdapterRequestArtifact>::derive(&request_bytes)?;
    let completion = if request.expected_outputs().is_empty() {
        CallAdapterCompletionV1::RejectedBeforeInvocation
    } else {
        CallAdapterCompletionV1::InvokedVoid
    };
    let result = CallAdapterResultV1::new(request_id, request.invocation(), completion, observed)?;
    write_output(
        &output_root,
        request.result_path().as_str(),
        &cairn_codec::to_vec(&result)?,
    )?;
    Ok(())
}

fn parse_arguments(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let values = arguments.into_iter().collect::<Vec<_>>();
    if values.len() != 4 || values[0] != "--request" || values[2] != "--output-root" {
        return Err("expected --request <path> --output-root <path>".into());
    }
    Ok((PathBuf::from(&values[1]), PathBuf::from(&values[3])))
}

fn validate_invocation(
    request_path: &Path,
    request: &CallAdapterRequestV1,
) -> Result<(), Box<dyn Error>> {
    let input_root = request_path
        .parent()
        .and_then(Path::parent)
        .ok_or("request path has no Cairn input root")?;
    let bytes = fs::read(input_root.join(request.invocation_path().as_str()))?;
    match request.invocation() {
        CorpusInvocationIdentityV1::ExecutableOracle { manifest } => {
            let _: ExecutableOracleInvocationV1 = cairn_codec::from_slice(&bytes)?;
            require_identity::<ExecutableOracleInvocationArtifact>(&bytes, manifest)
        }
        CorpusInvocationIdentityV1::Boundary { manifest } => {
            let _: MaterializedBoundaryCaseV1 = cairn_codec::from_slice(&bytes)?;
            require_identity::<MaterializedBoundaryCaseArtifact>(&bytes, manifest)
        }
        CorpusInvocationIdentityV1::InputValue { manifest } => {
            let _: MaterializedInputValueCaseV1 = cairn_codec::from_slice(&bytes)?;
            require_identity::<MaterializedInputValueCaseArtifact>(&bytes, manifest)
        }
        CorpusInvocationIdentityV1::MemorySurface { manifest } => {
            let _: MaterializedMemorySurfaceCaseV1 = cairn_codec::from_slice(&bytes)?;
            require_identity::<MaterializedMemorySurfaceCaseArtifact>(&bytes, manifest)
        }
    }
}

fn require_identity<T: ContentType>(
    bytes: &[u8],
    expected: ContentId<T>,
) -> Result<(), Box<dyn Error>> {
    if ContentId::<T>::derive(bytes)? != expected {
        return Err("invocation manifest identity mismatch".into());
    }
    Ok(())
}

fn write_output(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().ok_or("output path has no parent")?)?;
    fs::write(path, bytes)?;
    Ok(())
}
