use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bureau::config::Limits;
use bureau::state::{FreshClaim, LeaseOwner, Store};

pub const ASSIGNMENT: &str = "rate-limited";
pub const TTL: Duration = Duration::from_secs(60);
static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "bureau-rate-{}-{}-{tag}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            builder.mode(0o700);
        }
        builder.create(&path).expect("private rate fixture");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("rate fixture cleanup failed: {error}");
            assert!(std::thread::panicking(), "fixture cleanup must succeed");
        }
    }
}

pub struct Fixture {
    pub stores: [Arc<Store>; 2],
    directory: TestDir,
}

impl Fixture {
    pub fn new(tag: &str) -> Self {
        let directory = TestDir::new(tag);
        std::fs::create_dir(directory.path().join("runs")).expect("empty run history");
        let database = directory.path().join("state.db");
        let stores = std::array::from_fn(|_| {
            Arc::new(Store::open(&database).expect("independent database connection"))
        });
        Self { stores, directory }
    }

    pub fn owner(&self, connection: usize, item: &str, run: &str) -> LeaseOwner {
        LeaseOwner::new(
            self.stores[connection].clone(),
            ASSIGNMENT,
            "github",
            item,
            run,
        )
        .expect("run owner")
    }

    pub fn claim(&self, owner: &LeaseOwner, limits: &Limits) -> Option<FreshClaim> {
        owner
            .claim_fresh_with_limits(TTL, &self.directory.path().join("runs"), limits, 0)
            .expect("fresh admission")
    }
}

pub fn rate_limits() -> [Limits; 2] {
    [
        Limits {
            max_runs_per_hour: Some(1),
            ..Limits::default()
        },
        Limits {
            max_runs_per_day: Some(1),
            ..Limits::default()
        },
    ]
}
