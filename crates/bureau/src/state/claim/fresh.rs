use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, TransactionBehavior};

use super::super::{Error, Store, now_millis};
use super::LeaseOwner;
use crate::runlog::{FactoryHistory, FactorySource};

#[cfg(test)]
mod tests;

const LIVE_RUNS: &str = "SELECT run_id FROM leases WHERE expires_at_ms > ?1";
type Work = BTreeMap<String, String>;

fn live_runs(connection: &Connection, now: i64) -> Result<BTreeSet<String>, Error> {
    let mut statement = connection.prepare(LIVE_RUNS)?;
    statement
        .query_map([now], |row| row.get(0))?
        .collect::<Result<_, _>>()
        .map_err(Error::from)
}

enum Verification<T> {
    Refresh(FactorySource),
    Current(T),
}

fn prepared<T>(
    mut operation: impl FnMut(&mut Option<FactoryHistory>) -> Result<Verification<T>, Error>,
    mut before_replay: impl FnMut(),
) -> Result<T, Error> {
    let mut history = None;
    loop {
        match operation(&mut history)? {
            Verification::Current(value) => return Ok(value),
            Verification::Refresh(source) => {
                before_replay();
                history = Some(FactoryHistory::prepare(source, history.take()));
            }
        }
    }
}

fn verify(
    runs: &Path,
    prepared: &mut Option<FactoryHistory>,
) -> Result<Verification<FactoryHistory>, Error> {
    let source = prepared.as_ref().map_or_else(
        || FactorySource::read(runs),
        |history| history.capture(runs),
    )?;
    match prepared.take() {
        Some(history) if history.matches(&source) => Ok(Verification::Current(history)),
        previous => {
            *prepared = previous;
            Ok(Verification::Refresh(source))
        }
    }
}

fn inspect(
    connection: &Connection,
    runs: &Path,
    assignment: &str,
    forge: &str,
    prepared: &mut Option<FactoryHistory>,
) -> Result<Verification<Work>, Error> {
    let history = match verify(runs, prepared)? {
        Verification::Refresh(source) => return Ok(Verification::Refresh(source)),
        Verification::Current(history) => history,
    };
    let active = live_runs(connection, now_millis())?;
    Ok(Verification::Current(
        history.work(assignment, forge, &active)?,
    ))
}

impl Store {
    fn observation(
        &self,
        runs: &Path,
        assignment: &str,
        forge: &str,
        prepared: &mut Option<FactoryHistory>,
    ) -> Result<Verification<Work>, Error> {
        let mut connection = self.lock();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = inspect(&tx, runs, assignment, forge, prepared)?;
        tx.commit()?;
        drop(connection);
        Ok(result)
    }

    fn observe(
        &self,
        runs: &Path,
        assignment: &str,
        forge: &str,
        before_replay: impl FnMut(),
    ) -> Result<Work, Error> {
        prepared(
            |history| self.observation(runs, assignment, forge, history),
            before_replay,
        )
    }

    /// Reads factory exclusions and unpublished live owners under one admission fence.
    ///
    /// Expensive replay stays outside that fence; exact bytes and identities are revalidated.
    ///
    /// # Errors
    /// Propagates database failures and invalid or unowned incomplete run logs.
    pub fn preserved_factory_work(
        &self,
        runs: &Path,
        assignment: &str,
        forge: &str,
    ) -> Result<Work, Error> {
        self.observe(runs, assignment, forge, || {})
    }
}

/// A fresh run cannot replace unfinished local-factory work after its lease expires.
#[derive(Debug, PartialEq, Eq)]
pub enum FreshClaim {
    Claimed,
    Busy,
    PreservedFactory(String),
}

fn outcome(won: bool, reserved: Option<String>) -> FreshClaim {
    match reserved {
        Some(run) => FreshClaim::PreservedFactory(run),
        None if won => FreshClaim::Claimed,
        None => FreshClaim::Busy,
    }
}

fn checked_claim(
    won: bool,
    checked: Option<Verification<Work>>,
    item: &str,
) -> Result<Verification<FreshClaim>, Error> {
    match checked.ok_or_else(|| std::io::Error::other("admission predicate was not evaluated"))? {
        Verification::Refresh(source) => Ok(Verification::Refresh(source)),
        Verification::Current(mut work) => {
            Ok(Verification::Current(outcome(won, work.remove(item))))
        }
    }
}

impl LeaseOwner {
    fn claim_once(
        &self,
        ttl: Duration,
        runs: &Path,
        prepared: &mut Option<FactoryHistory>,
    ) -> Result<Verification<FreshClaim>, Error> {
        let mut checked = None;
        let won = self.store.claim_owner_if(self, ttl, |connection, _| {
            let result = inspect(
                connection,
                runs,
                &self.key.assignment,
                &self.key.forge,
                prepared,
            )?;
            let available = matches!(&result, Verification::Current(work)
                if !work.contains_key(&self.key.external_id));
            checked = Some(result);
            Ok(available)
        })?;
        checked_claim(won, checked, &self.key.external_id)
    }

    fn claim_fresh_with(
        &self,
        ttl: Duration,
        runs: &Path,
        before_replay: impl FnMut(),
    ) -> Result<FreshClaim, Error> {
        prepared(|history| self.claim_once(ttl, runs, history), before_replay)
    }

    /// Claims fresh work after exact fenced validation of replay prepared outside the fence.
    ///
    /// Existing-run recovery deliberately uses `claim` instead: this guards new run identities.
    ///
    /// # Errors
    /// Propagates database failures and unreadable or inconsistent authoritative logs.
    pub fn claim_fresh(&self, ttl: Duration, runs: &Path) -> Result<FreshClaim, Error> {
        self.claim_fresh_with(ttl, runs, || {})
    }
}
