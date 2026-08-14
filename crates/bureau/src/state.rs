//! Layer 5: durable state (DESIGN.md section 7) — `SQLite`, two concerns.
//!
//! There is no queue table: pending work is a query against the forge,
//! not stored state. What survives between runs is exactly:
//!
//! - **leases** — claim records with expiry. Single-claim is enforced by
//!   a unique index inside a transaction, never an in-process mutex, so
//!   two daemons on two machines arbitrate through the database alone.
//! - **budget counters** — run history for limits checked *before* spawn.
//! - **dedup markers** — content hashes of proposed output, so a
//!   scheduled pipeline never re-proposes an identical change.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::Connection;

use crate::config::Limits;

/// State-layer failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `SQLite` failure.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// Filesystem failure opening the database file.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A claim on one work item, with expiry. A crashed run releases
/// automatically when the lease expires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    /// The assignment holding the claim.
    pub assignment: String,
    /// The forge the item lives on (`ado` / `github`).
    pub forge: String,
    /// The item's id on that forge.
    pub external_id: String,
    /// Expiry, milliseconds since the Unix epoch.
    pub expires_at_ms: u64,
}

/// How a dedup marker was resolved when it was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// A PR was opened for this content.
    Proposed,
    /// The proposal was rejected (review closed it).
    Rejected,
}

impl Disposition {
    /// The stored token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Rejected => "rejected",
        }
    }
}

/// The durable store. Safe to share: the connection sits behind a mutex,
/// and single-claim correctness lives in the database, not the process.
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Opens (creating) the database at `path`, applying the schema.
    ///
    /// # Errors
    /// Propagates filesystem and `SQLite` failures.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let _ = path;
        todo!(
            "open db, create tables: leases (unique assignment+forge+external_id), runs (assignment, started_at_ms, cost_usd), dedup (content_hash, disposition, at_ms)"
        )
    }

    /// An in-memory store, for tests.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn open_in_memory() -> Result<Self, Error> {
        todo!()
    }

    /// Attempts to claim an item. Compare-and-swap: returns `Ok(false)`
    /// when another live lease holds the item. Expired leases are
    /// reclaimed by the caller of this method.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn try_claim(
        &self,
        assignment: &str,
        forge: &str,
        external_id: &str,
        ttl: Duration,
    ) -> Result<bool, Error> {
        let _conn = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = (assignment, forge, external_id, ttl);
        todo!()
    }

    /// Releases a claim. Idempotent.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn release(&self, assignment: &str, external_id: &str) -> Result<(), Error> {
        let _ = (assignment, external_id);
        todo!()
    }

    /// Extends a live lease. Returns `Ok(false)` when the lease is gone
    /// or already expired.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn renew(&self, assignment: &str, external_id: &str, ttl: Duration) -> Result<bool, Error> {
        let _ = (assignment, external_id, ttl);
        todo!()
    }

    /// Live leases for an assignment (expired ones excluded).
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn active(&self, assignment: &str) -> Result<Vec<Lease>, Error> {
        let _ = assignment;
        todo!()
    }

    /// How many more runs the assignment may start now: the minimum
    /// remaining headroom across `max_concurrent` (live leases),
    /// `max_runs_per_hour`, `max_runs_per_day`, `max_open_prs`, and
    /// `max_cost_per_day_usd`. Zero when any limit is hit. Checked
    /// *before* anything spawns.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn headroom(
        &self,
        assignment: &str,
        limits: &Limits,
        open_prs: usize,
    ) -> Result<usize, Error> {
        let _ = (assignment, limits, open_prs);
        todo!()
    }

    /// Records a completed run's cost for budget accounting.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn record_run(&self, assignment: &str, cost_usd: f64) -> Result<(), Error> {
        let _ = (assignment, cost_usd);
        todo!()
    }

    /// Whether an identical proposal is already open or was previously
    /// rejected.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn seen(&self, content_hash: &str) -> Result<bool, Error> {
        let _ = content_hash;
        todo!()
    }

    /// Writes a dedup marker for a content hash. Idempotent.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn mark_seen(&self, content_hash: &str, disposition: Disposition) -> Result<(), Error> {
        let _ = (content_hash, disposition);
        todo!()
    }
}
