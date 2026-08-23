//! Layer 0: the bytes handed to the run-log sink must be decodable as they
//! arrive (DESIGN.md section 12; offline, `/bin/sh` only).
//!
//! The sink turns bytes into an `output` event's text. A read boundary can
//! fall inside a multi-byte character, and a sink that decodes half of one
//! writes a replacement character and loses the other half — which is how
//! captured agent transcripts ended up full of mojibake. So the spawn layer
//! withholds an unfinished character until the rest of it arrives.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bureau::process::{SpawnRequest, shared_log, spawn};

/// 4000 three-byte characters. Reads are a power of two and never a multiple
/// of three, so a boundary is guaranteed to land inside one of them.
const REPEATS: usize = 4000;
const SCRIPT: &str = r#"printf "\342\227\217%.0s" $(seq 4000)"#;

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("bureau-utf8-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A sink that decodes each write on its own, exactly as the run log does.
#[derive(Clone, Default)]
struct TextLog(Arc<Mutex<String>>);

impl TextLog {
    fn text(&self) -> String {
        self.0.lock().expect("text log lock").clone()
    }
}

impl Write for TextLog {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .expect("text log lock")
            .push_str(&String::from_utf8_lossy(buf));
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn request(dir: &Path, log: &TextLog) -> SpawnRequest {
    SpawnRequest {
        argv: vec!["sh".to_owned(), "-c".to_owned(), SCRIPT.to_owned()],
        dir: dir.to_path_buf(),
        env: BTreeMap::new(),
        stdin: Vec::new(),
        timeout: Duration::from_secs(30),
        secrets: Vec::new(),
        log: Some(shared_log(log.clone())),
        cancel: None,
    }
}

#[tokio::test]
async fn a_character_split_across_a_read_boundary_reaches_the_sink_whole() {
    let dir = TestDir::new();
    let log = TextLog::default();
    let result = spawn(request(dir.path(), &log)).await;
    let text = log.text();

    assert_eq!(
        (
            text.matches('\u{25cf}').count(),
            text.matches('\u{fffd}').count(),
            result.stdout.len(),
        ),
        (REPEATS, 0, REPEATS * 3)
    );
}
