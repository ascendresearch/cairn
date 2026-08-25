use std::{env, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    match cairn_server::run_from_arguments(env::args_os()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cairn-server: {error}");
            ExitCode::FAILURE
        }
    }
}
