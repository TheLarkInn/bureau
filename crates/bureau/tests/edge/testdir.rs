//! A per-test temporary directory, removed on drop.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

/// A unique temporary directory removed on drop.
pub struct TestDir(PathBuf);

impl TestDir {
    /// Creates a fresh directory under the OS temp dir.
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bureau-edge-test-{}-{}-{tag}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    /// The directory's path.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
