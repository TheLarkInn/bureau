use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

use super::super::super::{limits, sql, usage};
use super::{Error, FreshClaim, LeaseOwner};
use crate::config::Limits;

#[cfg(test)]
mod tests;

fn unseen(connection: &Connection, content_hash: Option<&str>) -> Result<bool, Error> {
    match content_hash {
        Some(hash) => Ok(!connection.query_row(sql::SEEN, (hash,), |row| row.get::<_, bool>(0))?),
        None => Ok(true),
    }
}

impl LeaseOwner {
    fn within_limits(
        &self,
        connection: &Connection,
        now: i64,
        limits: &Limits,
        open_prs: usize,
    ) -> Result<bool, Error> {
        let (live, hour, day, spent) = usage(connection, &self.key.assignment, now)?;
        Ok(limits::remaining(limits, open_prs, live, hour, day, spent) > 0)
    }

    fn claim_limited(
        &self,
        ttl: Duration,
        runs: &Path,
        limits: &Limits,
        open_prs: usize,
        content_hash: Option<&str>,
    ) -> Result<Option<FreshClaim>, Error> {
        let mut exhausted = false;
        let claim = self.claim_fresh_if(
            ttl,
            runs,
            |connection, now| {
                if !unseen(connection, content_hash)? {
                    return Ok(false);
                }
                exhausted = !self.within_limits(connection, now, limits, open_prs)?;
                Ok(!exhausted)
            },
            || {},
        )?;
        Ok((!exhausted).then_some(claim))
    }

    /// Rechecks durable assignment usage and claims the item in one write transaction.
    ///
    /// Returns `None` when a configured limit is exhausted. `open_prs` is the
    /// caller's forge observation; this does not reserve future PRs or unmeasured cost.
    /// Explicit run/retry may repeat seen content; existing-run recovery uses `claim`.
    ///
    /// # Errors
    /// Propagates database failures and unreadable or inconsistent authoritative logs.
    pub fn claim_fresh_with_limits(
        &self,
        ttl: Duration,
        runs: &Path,
        limits: &Limits,
        open_prs: usize,
    ) -> Result<Option<FreshClaim>, Error> {
        self.claim_limited(ttl, runs, limits, open_prs, None)
    }

    /// Seen content is an item exclusion, not an admission or exhausted capacity.
    /// The check shares the write fence with the lease and immutable rate charge.
    pub(crate) fn claim_fresh_unseen_with_limits(
        &self,
        ttl: Duration,
        runs: &Path,
        limits: &Limits,
        open_prs: usize,
        content_hash: &str,
    ) -> Result<Option<FreshClaim>, Error> {
        self.claim_limited(ttl, runs, limits, open_prs, Some(content_hash))
    }
}
