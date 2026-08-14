//! The command line. Verbs are verbs; the hard cap is 15 (DESIGN.md
//! sections 2–3). This session ships `validate`, `version`, and the
//! `fake` adapter testing seam.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context as _;
use clap::{Parser, Subcommand};

use bureau::adapters::fake::{self, Transcript};
use bureau::config::Config;
use bureau::process::SpawnRequest;

/// How long `fake record` lets a subprocess run before killing it.
const RECORD_TIMEOUT: Duration = Duration::from_secs(3600);

/// `bureau` — a local agent work runner.
#[derive(Debug, Parser)]
#[command(name = "bureau", version, about)]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub verb: Verb,
}

/// CLI verbs.
#[derive(Debug, Subcommand)]
pub enum Verb {
    /// Checks a config directory and reports every error in one pass.
    Validate {
        /// Path to the config repository checkout.
        #[arg(default_value = "runner-config")]
        dir: PathBuf,
    },
    /// Prints the version.
    Version,
    /// Replays or records adapter transcripts — the testing seam.
    Fake {
        /// What to do with the fixture.
        #[command(subcommand)]
        action: FakeAction,
    },
}

/// `fake` adapter operations.
#[derive(Debug, Subcommand)]
pub enum FakeAction {
    /// Replays a transcript fixture to stdout/stderr, exiting with the
    /// recorded exit code.
    Replay {
        /// Fixture path.
        fixture: PathBuf,
    },
    /// Runs a command under the layer-0 contract and writes the captured
    /// transcript fixture, passing output through.
    Record {
        /// Fixture path to write.
        fixture: PathBuf,
        /// The command to run, after `--`.
        #[arg(last = true, required = true)]
        argv: Vec<String>,
    },
}

/// Runs the CLI and returns the process exit code.
///
/// # Errors
/// Propagates unexpected failures (fixture I/O, serialization).
pub async fn run(cli: Cli) -> anyhow::Result<i32> {
    match cli.verb {
        Verb::Version => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Verb::Validate { dir } => Ok(validate(&dir)),
        Verb::Fake { action } => fake_action(action).await,
    }
}

fn validate(dir: &std::path::Path) -> i32 {
    match Config::load(dir) {
        Ok(config) => {
            println!(
                "config ok: {} repos, {} roles, {} assignments",
                config.repos.len(),
                config.roles.len(),
                config.assignments.len()
            );
            0
        }
        Err(errors) => {
            for error in &errors {
                eprintln!("{error}");
            }
            eprintln!("{} config error(s)", errors.len());
            1
        }
    }
}

async fn fake_action(action: FakeAction) -> anyhow::Result<i32> {
    match action {
        FakeAction::Replay { fixture } => {
            let transcript = Transcript::load(&fixture).context("loading fixture")?;
            Ok(fake::replay(&transcript).await)
        }
        FakeAction::Record { fixture, argv } => record(&fixture, argv).await,
    }
}

async fn record(fixture: &std::path::Path, argv: Vec<String>) -> anyhow::Result<i32> {
    let dir = std::env::current_dir().context("reading current directory")?;
    let request = SpawnRequest {
        argv,
        dir,
        env: BTreeMap::new(),
        stdin: Vec::new(),
        timeout: RECORD_TIMEOUT,
        secrets: Vec::new(),
        log: None,
    };
    let transcript = fake::record(request).await;
    transcript.save(fixture).context("writing fixture")?;
    Ok(fake::replay(&transcript).await)
}
