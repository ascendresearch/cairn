//! Deterministic identity build fixture for admission-variant execution tests.
//!
//! It validates that the supplied implementation bytes match the variant and emits those same
//! bytes as the call-adapter executable. It proves build protocol composition, not compilation.

use std::{error::Error, fs, path::PathBuf};

use cairn_protocol::ContentId;
use cairn_verification::{ImplementationBundleArtifact, ImplementationVariantV1};

fn main() {
    if let Err(error) = run(std::env::args_os().skip(1)) {
        eprintln!("variant build fixture failed: {error}");
        std::process::exit(1);
    }
}

fn run(arguments: impl IntoIterator<Item = std::ffi::OsString>) -> Result<(), Box<dyn Error>> {
    let values = arguments.into_iter().collect::<Vec<_>>();
    if values.len() != 6
        || values[0] != "--variant"
        || values[2] != "--implementation"
        || values[4] != "--output"
    {
        return Err("expected --variant <path> --implementation <path> --output <path>".into());
    }
    let variant_bytes = fs::read(PathBuf::from(&values[1]))?;
    let variant: ImplementationVariantV1 = cairn_codec::from_slice(&variant_bytes)?;
    let implementation = fs::read(PathBuf::from(&values[3]))?;
    if ContentId::<ImplementationBundleArtifact>::derive(&implementation)?
        != variant.implementation()
    {
        return Err("implementation bundle identity mismatch".into());
    }
    let output = PathBuf::from(&values[5]);
    fs::create_dir_all(output.parent().ok_or("output path has no parent")?)?;
    fs::write(output, implementation)?;
    Ok(())
}
