use std::{env, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    if let Err(error) = cairn_observability::init("cairn-server") {
        eprintln!("cairn-server logging initialization failed: {error}");
        return ExitCode::FAILURE;
    }
    match cairn_server::run_from_arguments(env::args_os()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(
                target: "cairn.server",
                event = "process_failed",
                error = %error,
                "cairn server terminated with an error"
            );
            ExitCode::FAILURE
        }
    }
}
