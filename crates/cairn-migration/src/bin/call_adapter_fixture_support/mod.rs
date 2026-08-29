//! Shared deterministic host implementation of the isolated call-adapter process protocol.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use cairn_migration::{
    CallAdapterCompletionV1, CallAdapterObservedOutputV1, CallAdapterRequestArtifact,
    CallAdapterRequestV1, CallAdapterResultV1, CollectionF32InputBytesArtifact,
    CollectionF32InvocationArtifact, CollectionF32InvocationV1,
    CollectionF32ThresholdBytesArtifact, CorpusInvocationIdentityV1,
    ExecutableOracleInvocationArtifact, ExecutableOracleInvocationV1,
    MaterializedBoundaryCaseArtifact, MaterializedBoundaryCaseV1,
    MaterializedInputValueCaseArtifact, MaterializedInputValueCaseV1,
    MaterializedMemorySurfaceCaseArtifact, MaterializedMemorySurfaceCaseV1,
};
use cairn_protocol::{ContentId, ContentType};

#[allow(dead_code, reason = "shared by the non-collection fixture binaries")]
pub fn main_with_output_byte(output_byte: u8) {
    if let Err(error) = run(std::env::args_os().skip(1), output_byte) {
        eprintln!("call-adapter fixture failed: {error}");
        std::process::exit(1);
    }
}

#[allow(dead_code, reason = "shared by the collection fixture binary")]
pub fn main_collection_f32_reversed() {
    if let Err(error) = run_collection_f32(std::env::args_os().skip(1), false) {
        eprintln!("collection call-adapter fixture failed: {error}");
        std::process::exit(1);
    }
}

#[allow(dead_code, reason = "shared by the missing-occurrence fixture binary")]
pub fn main_collection_f32_missing_occurrence() {
    if let Err(error) = run_collection_f32(std::env::args_os().skip(1), true) {
        eprintln!("missing-occurrence call-adapter fixture failed: {error}");
        std::process::exit(1);
    }
}

#[allow(dead_code, reason = "shared by the collection fixture binaries")]
fn run_collection_f32(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
    drop_last_selected: bool,
) -> Result<(), Box<dyn Error>> {
    let (request_path, output_root) = parse_arguments(arguments)?;
    let request_bytes = fs::read(&request_path)?;
    let request: CallAdapterRequestV1 = cairn_codec::from_slice(&request_bytes)?;
    let input_root = request_path
        .parent()
        .and_then(Path::parent)
        .ok_or("request path has no Cairn input root")?;
    let invocation_bytes = fs::read(input_root.join(request.invocation_path().as_str()))?;
    let CorpusInvocationIdentityV1::CollectionOutput { manifest } = request.invocation() else {
        return Err("collection fixture requires a collection-output invocation".into());
    };
    let invocation: CollectionF32InvocationV1 = cairn_codec::from_slice(&invocation_bytes)?;
    require_identity::<CollectionF32InvocationArtifact>(&invocation_bytes, manifest)?;

    let input_bytes = fs::read(input_root.join(invocation.input().path().as_str()))?;
    require_identity::<CollectionF32InputBytesArtifact>(&input_bytes, invocation.input().bytes())?;
    let threshold_bytes = fs::read(input_root.join(invocation.threshold().path().as_str()))?;
    require_identity::<CollectionF32ThresholdBytesArtifact>(
        &threshold_bytes,
        invocation.threshold().bytes(),
    )?;
    if input_bytes.len() != usize::try_from(invocation.input().byte_length().get())?
        || threshold_bytes.len() != 4
    {
        return Err("collection fixture input length mismatch".into());
    }
    let threshold = f32::from_bits(u32::from_le_bytes(threshold_bytes.as_slice().try_into()?));
    let mut selected = input_bytes
        .chunks_exact(4)
        .filter_map(|chunk| {
            let bytes = <[u8; 4]>::try_from(chunk).expect("chunks_exact fixes f32 width");
            (f32::from_bits(u32::from_le_bytes(bytes)) > threshold).then_some(bytes)
        })
        .collect::<Vec<_>>();
    selected.reverse();
    if drop_last_selected {
        selected.pop();
    }

    let mut values = vec![0_u8; usize::try_from(invocation.values_output().byte_length().get())?];
    for (destination, source) in values.chunks_exact_mut(4).zip(&selected) {
        destination.copy_from_slice(source);
    }
    let count = u32::try_from(selected.len())?.to_le_bytes().to_vec();
    let outputs = [
        (invocation.values_output(), values),
        (invocation.count_output(), count),
    ];
    if request.expected_outputs().len() != outputs.len() {
        return Err("collection fixture output declaration mismatch".into());
    }
    let mut observed = Vec::with_capacity(outputs.len());
    for (declared, bytes) in outputs {
        let expected = request
            .expected_outputs()
            .iter()
            .find(|expected| expected.argument_index() == declared.argument_index())
            .ok_or("collection fixture output argument missing")?;
        if expected.buffer() != declared.buffer()
            || expected.byte_length() != declared.byte_length()
            || u64::try_from(bytes.len())? != declared.byte_length().get()
        {
            return Err("collection fixture output metadata mismatch".into());
        }
        write_output(&output_root, expected.path().as_str(), &bytes)?;
        observed.push(CallAdapterObservedOutputV1::from_bytes(
            expected.argument_index(),
            expected.buffer().clone(),
            &bytes,
        )?);
    }
    observed.sort_by_key(CallAdapterObservedOutputV1::argument_index);
    let result = CallAdapterResultV1::new(
        ContentId::<CallAdapterRequestArtifact>::derive(&request_bytes)?,
        request.invocation(),
        CallAdapterCompletionV1::InvokedVoid,
        observed,
    )?;
    write_output(
        &output_root,
        request.result_path().as_str(),
        &cairn_codec::to_vec(&result)?,
    )?;
    Ok(())
}

#[allow(dead_code, reason = "shared by the non-collection fixture binaries")]
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

#[allow(dead_code, reason = "shared by the non-collection fixture binaries")]
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
        CorpusInvocationIdentityV1::CollectionOutput { manifest } => {
            let _: CollectionF32InvocationV1 = cairn_codec::from_slice(&bytes)?;
            require_identity::<CollectionF32InvocationArtifact>(&bytes, manifest)
        }
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
