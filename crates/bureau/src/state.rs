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

mod disposition;
mod lease;
mod limits;
mod project;
mod sql;

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

pub use disposition::Disposition;
pub use lease::maintain as maintain_lease;
pub use project::{run as project_run, terminal as project_terminal};

use crate::config::Limits;

/// One hour in milliseconds; the hourly run-count window.
const HOUR_MS: i64 = 3_600_000;
/// One day in milliseconds; the daily run-count and cost windows.
const DAY_MS: i64 = 86_400_000;

/// State-layer failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `SQLite` failure.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// Filesystem failure opening the database file.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A stored dedup token this build does not recognize.
    #[error("unknown stored disposition: {0}")]
    UnknownDisposition(String),
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
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        Self::init(Connection::open(path)?)
    }

    /// An in-memory store, for tests.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn open_in_memory() -> Result<Self, Error> {
        Self::init(Connection::open_in_memory()?)
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
        let now = now_millis();
        let expires = now.saturating_add(duration_millis(ttl));
        claim_tx(
            &mut self.lock(),
            assignment,
            forge,
            external_id,
            now,
            expires,
        )
    }

    /// Releases a claim. Idempotent.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn release(&self, assignment: &str, external_id: &str) -> Result<(), Error> {
        self.lock()
            .execute(sql::RELEASE, (assignment, external_id))?;
        Ok(())
    }

    /// Extends a live lease. Returns `Ok(false)` when the lease is gone
    /// or already expired.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn renew(&self, assignment: &str, external_id: &str, ttl: Duration) -> Result<bool, Error> {
        let now = now_millis();
        let expires = now.saturating_add(duration_millis(ttl));
        let changed = self
            .lock()
            .execute(sql::RENEW, (expires, assignment, external_id, now))?;
        Ok(changed > 0)
    }

    /// Live leases for an assignment (expired ones excluded).
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn active(&self, assignment: &str) -> Result<Vec<Lease>, Error> {
        active_leases(&self.lock(), assignment)
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
        let now = now_millis();
        let (live, hour, day, spent) = usage(&self.lock(), assignment, now)?;
        Ok(limits::remaining(limits, open_prs, live, hour, day, spent))
    }

    /// Records a completed run's cost for budget accounting.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn record_run(&self, run_id: &str, assignment: &str, cost_usd: f64) -> Result<(), Error> {
        let params = (run_id, assignment, now_millis(), cost_usd);
        self.lock().execute(sql::RECORD_RUN, params)?;
        Ok(())
    }

    /// Whether an identical proposal is already open or was previously
    /// rejected.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn seen(&self, content_hash: &str) -> Result<bool, Error> {
        let seen = self
            .lock()
            .query_row(sql::SEEN, (content_hash,), |row| row.get(0))?;
        Ok(seen)
    }

    /// Writes a dedup marker for a content hash. Idempotent — except a
    /// terminal `Rejected` row is never overwritten; weaker markers flip.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn mark_seen(&self, content_hash: &str, disposition: Disposition) -> Result<(), Error> {
        let params = (
            content_hash,
            disposition.as_str(),
            now_millis(),
            Disposition::Rejected.as_str(),
        );
        self.lock().execute(sql::MARK_SEEN, params)?;
        Ok(())
    }

    /// The disposition recorded for a content hash, when one exists.
    ///
    /// # Errors
    /// Propagates `SQLite` failures and rejects unknown stored tokens.
    pub fn disposition(&self, content_hash: &str) -> Result<Option<Disposition>, Error> {
        sql::disposition(&self.lock(), content_hash)?
            .map(|token| Disposition::from_token(&token))
            .transpose()
    }

    /// Applies the schema to a connection behind the sharing mutex.
    fn init(conn: Connection) -> Result<Self, Error> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(sql::SCHEMA)?;
        sql::migrate_runs(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Locks the connection, recovering from a poisoned mutex.
    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Live leases for an assignment, read through the locked connection.
fn active_leases(conn: &Connection, assignment: &str) -> Result<Vec<Lease>, Error> {
    let mut stmt = conn.prepare(sql::ACTIVE_LEASES)?;
    let rows = stmt.query_map((assignment, now_millis()), sql::lease_from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// One moment's usage: live leases, runs this hour and day, day's spend.
fn usage(conn: &Connection, assignment: &str, now: i64) -> Result<(u32, u32, u32, f64), Error> {
    let live = sql::count(conn, sql::LIVE_LEASES, assignment, now)?;
    let hour = sql::count(conn, sql::RUNS_SINCE, assignment, now - HOUR_MS)?;
    let day = sql::count(conn, sql::RUNS_SINCE, assignment, now - DAY_MS)?;
    let spent = sql::cost_since(conn, assignment, now - DAY_MS)?;
    Ok((live, hour, day, spent))
}

/// One claim transaction: reap the key's expired lease, then insert.
/// The unique index makes exactly one concurrent claimant win.
fn claim_tx(
    conn: &mut Connection,
    assignment: &str,
    forge: &str,
    external_id: &str,
    now: i64,
    expires: i64,
) -> Result<bool, Error> {
    let tx = conn.transaction()?;
    tx.execute(sql::REAP_EXPIRED, (assignment, forge, external_id, now))?;
    match tx.execute(sql::INSERT_LEASE, (assignment, forge, external_id, expires)) {
        Ok(_) => {
            tx.commit()?;
            Ok(true)
        }
        Err(err) if sql::is_unique_violation(&err) => Ok(false),
        Err(err) => Err(err.into()),
    }
}

/// Milliseconds since the Unix epoch, clamped into `i64`.
fn now_millis() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    i64::try_from(millis).unwrap_or(i64::MAX)
}

/// A duration as whole milliseconds, clamped into `i64`.
fn duration_millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}
