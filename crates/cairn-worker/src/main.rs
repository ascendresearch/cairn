use std::{env, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    if let Err(error) = cairn_observability::init("cairn-worker") {
        eprintln!("cairn-worker logging initialization failed: {error}");
        return ExitCode::FAILURE;
    }
    match cairn_worker::run_from_arguments(env::args_os()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(
                target: "cairn.worker",
                event = "process_failed",
                error = %error,
                "cairn worker terminated with an error"
            );
            ExitCode::FAILURE
        }
    }
}
