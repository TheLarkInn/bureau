//! Binary entry point. Library code returns typed errors; `anyhow`
//! context is added here at the binary boundary only.

mod cli;

use std::process::ExitCode;

use clap::Parser as _;

#[tokio::main]
async fn main() -> ExitCode {
    match cli::run(cli::Cli::parse()).await {
        Ok(code) => ExitCode::from(u8::try_from(code.clamp(0, 255)).unwrap_or(1)),
        Err(error) => {
            cli::out::error(format_args!("{error:#}"));
            ExitCode::from(2)
        }
    }
}
