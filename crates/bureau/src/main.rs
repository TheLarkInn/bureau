//! Binary entry point. Library code returns typed errors; `anyhow`
//! context is added here at the binary boundary only. `main` is also
//! the binary's only printing boundary: the verbs collect lines, the
//! loop below writes them.

mod cli;

use std::collections::BTreeMap;
use std::io::Write as _;
use std::process::ExitCode;

use clap::Parser as _;

#[tokio::main]
async fn main() -> ExitCode {
    // The composition root reads the process environment and the system
    // clock exactly once; everything below receives both as parameters.
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let clock: fn() -> u64 = || {
        let now = std::time::SystemTime::now();
        let since = now
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
    };
    let (lines, code) = match cli::run(cli::Cli::parse(), env, clock).await {
        Ok(done) => done,
        Err(error) => (vec![cli::Line::Err(format!("{error:#}"))], 2),
    };
    let (mut out, mut err) = (std::io::stdout().lock(), std::io::stderr().lock());
    for line in lines {
        let _ = match line {
            cli::Line::Out(text) => writeln!(out, "{text}"),
            cli::Line::Err(text) => writeln!(err, "{text}"),
        };
    }
    ExitCode::from(u8::try_from(code.clamp(0, 255)).unwrap_or(1))
}
