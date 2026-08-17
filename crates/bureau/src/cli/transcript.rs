//! Fake-adapter transcript replay and recording commands.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use anyhow::Context as _;
use bureau::adapters::fake::{self, Transcript};
use bureau::process::SpawnRequest;

use super::FakeAction;

const RECORD_TIMEOUT: Duration = Duration::from_secs(3600);

async fn record(fixture: &Path, argv: Vec<String>) -> anyhow::Result<i32> {
    let dir = std::env::current_dir().context("reading current directory")?;
    let request = SpawnRequest {
        argv,
        dir,
        env: BTreeMap::new(),
        stdin: Vec::new(),
        timeout: RECORD_TIMEOUT,
        secrets: Vec::new(),
        log: None,
        cancel: None,
    };
    let transcript = fake::record(request).await;
    transcript.save(fixture).context("writing fixture")?;
    Ok(fake::replay(&transcript).await)
}

pub(super) async fn run(action: FakeAction) -> anyhow::Result<i32> {
    match action {
        FakeAction::Replay { fixture } => {
            let transcript = Transcript::load(&fixture).context("loading fixture")?;
            Ok(fake::replay(&transcript).await)
        }
        FakeAction::Record { fixture, argv } => record(&fixture, argv).await,
    }
}
