use std::process::ExitCode;

use clap::Parser;
use rustling_cli::Cli;

#[tokio::main]
async fn main() -> ExitCode {
    match rustling_cli::execute(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rustlingpdf: {error}");
            error.exit_code()
        }
    }
}
