//! Isolated one-shot SIR process adapter.

use std::io::{Read, Write};

use cairn_migration::{SirProcessRequestV1, process_recorded_sir_request};

const MAX_REQUEST_BYTES: u64 = 4 * 1_024 * 1_024;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args_os().len() != 1 {
        return Err("cairn-sir accepts one canonical V1 request on stdin".into());
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_REQUEST_BYTES {
        return Err("cairn-sir request exceeds the current-V1 byte limit".into());
    }
    let request: SirProcessRequestV1 = cairn_codec::from_slice(&bytes)?;
    let terminal = process_recorded_sir_request(&request)?;
    std::io::stdout().write_all(&cairn_codec::to_vec(&terminal)?)?;
    Ok(())
}
