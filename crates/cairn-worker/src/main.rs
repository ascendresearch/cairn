use std::{env, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    match cairn_worker::run_from_arguments(env::args_os()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cairn-worker: {error}");
            ExitCode::FAILURE
        }
    }
}
