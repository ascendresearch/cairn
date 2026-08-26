//! Shared isolated-process entry point for compiled historical reduction fixtures.

use std::{error::Error, fs, path::PathBuf};

use cairn_migration::{
    HistoricalReductionAlgorithm, HistoricalReductionCorpusV1,
    compute_historical_reduction_fixture_output,
};

pub fn main_for(compiled_algorithm: HistoricalReductionAlgorithm) {
    if let Err(error) = run(std::env::args_os().skip(1), compiled_algorithm) {
        eprintln!("historical reduction fixture failed: {error}");
        std::process::exit(1);
    }
}

fn run(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
    compiled_algorithm: HistoricalReductionAlgorithm,
) -> Result<(), Box<dyn Error>> {
    let values = arguments.into_iter().collect::<Vec<_>>();
    if values.len() != 6
        || values[0] != "--corpus"
        || values[2] != "--algorithm"
        || values[4] != "--output"
    {
        return Err("expected --corpus <path> --algorithm <name> --output <path>".into());
    }
    let requested = values[3]
        .to_str()
        .ok_or("algorithm is not valid UTF-8")?
        .parse::<HistoricalReductionAlgorithm>()?;
    if requested != compiled_algorithm {
        return Err("requested algorithm differs from the compiled implementation".into());
    }
    let corpus_bytes = fs::read(PathBuf::from(&values[1]))?;
    let corpus: HistoricalReductionCorpusV1 = cairn_codec::from_slice(&corpus_bytes)?;
    let output = compute_historical_reduction_fixture_output(&corpus, compiled_algorithm)?;
    let output_path = PathBuf::from(&values[5]);
    fs::create_dir_all(output_path.parent().ok_or("output path has no parent")?)?;
    fs::write(output_path, cairn_codec::to_vec(&output)?)?;
    Ok(())
}
