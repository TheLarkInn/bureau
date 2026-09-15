use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, TransactionBehavior};

use super::super::{Error, Store, now_millis};
use super::LeaseOwner;

const LIVE_RUNS: &str = "SELECT run_id FROM leases WHERE expires_at_ms > ?1";

fn live_runs(connection: &Connection, now: i64) -> Result<BTreeSet<String>, Error> {
    let mut statement = connection.prepare(LIVE_RUNS)?;
    statement
        .query_map([now], |row| row.get(0))?
        .collect::<Result<_, _>>()
        .map_err(Error::from)
}

impl Store {
    /// Reads factory exclusions and unpublished live owners under one admission fence.
    ///
    /// # Errors
    /// Propagates database failures and invalid or unowned incomplete run logs.
    pub fn preserved_factory_work(
        &self,
        runs_dir: &Path,
        assignment: &str,
        forge: &str,
    ) -> Result<BTreeMap<String, String>, Error> {
        let mut connection = self.lock();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let active = live_runs(&tx, now_millis())?;
        let work = crate::runlog::preserved_factory_work_with_active(
            runs_dir, assignment, forge, &active,
        )?;
        tx.commit()?;
        drop(connection);
        Ok(work)
    }
}

/// A fresh run cannot replace unfinished local-factory work after its lease expires.
#[derive(Debug, PartialEq, Eq)]
pub enum FreshClaim {
    Claimed,
    Busy,
    PreservedFactory(String),
}

impl LeaseOwner {
    /// Claims fresh work only after a fenced, current read of authoritative factory events.
    ///
    /// Existing-run recovery deliberately uses `claim` instead: this guards new run identities.
    ///
    /// # Errors
    /// Propagates database failures and unreadable or inconsistent authoritative logs.
    pub fn claim_fresh(&self, ttl: Duration, runs_dir: &Path) -> Result<FreshClaim, Error> {
        let mut preserved = None;
        let won = self.store.claim_owner_if(self, ttl, |connection, now| {
            let active = live_runs(connection, now)?;
            let work = crate::runlog::preserved_factory_work_with_active(
                runs_dir,
                &self.key.assignment,
                &self.key.forge,
                &active,
            )?;
            preserved = work.get(&self.key.external_id).cloned();
            Ok(preserved.is_none())
        })?;
        Ok(match preserved {
            Some(run) => FreshClaim::PreservedFactory(run),
            None if won => FreshClaim::Claimed,
            None => FreshClaim::Busy,
        })
    }
}
