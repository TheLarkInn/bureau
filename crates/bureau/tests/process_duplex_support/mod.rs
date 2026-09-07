use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bureau::process::{SpawnRequest, shared_log};

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

pub struct TestDir(pub PathBuf);

impl TestDir {
    pub fn new(tag: &str) -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/process-duplex-tests")
            .join(format!(
                "{tag}-{}-{}",
                std::process::id(),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }

    pub fn request(&self, script: &str) -> SpawnRequest {
        SpawnRequest {
            argv: ["/bin/sh", "-c", script].map(str::to_owned).to_vec(),
            dir: self.0.clone(),
            env: BTreeMap::new(),
            stdin: Vec::new(),
            timeout: Duration::from_secs(5),
            secrets: Vec::new(),
            log: Some(shared_log(MemLog::default())),
            cancel: None,
        }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove test directory");
    }
}

#[derive(Clone, Default)]
pub struct MemLog(pub Arc<Mutex<Vec<u8>>>);

impl std::io::Write for MemLog {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log lock").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub async fn ready(dir: &TestDir) {
    for _ in 0..200 {
        if dir.0.join("ready").exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("process did not become ready");
}
