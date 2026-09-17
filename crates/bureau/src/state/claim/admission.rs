use std::time::Duration;

use rusqlite::{Connection, Transaction, TransactionBehavior};

use super::super::{Error, accounting, duration_millis, now_millis, sql};

#[cfg(test)]
mod tests;

pub(super) struct Claim<'a> {
    pub(super) assignment: &'a str,
    pub(super) forge: &'a str,
    pub(super) external_id: &'a str,
    pub(super) run_id: &'a str,
    pub(super) owner_id: &'a str,
}

fn commit_claim(tx: Transaction<'_>, claim: &Claim<'_>, now: i64) -> Result<bool, Error> {
    accounting::record(
        &tx,
        claim.assignment,
        claim.forge,
        claim.external_id,
        claim.run_id,
        now,
    )?;
    tx.commit()?;
    Ok(true)
}

fn insert_claim(
    tx: Transaction<'_>,
    claim: &Claim<'_>,
    now: i64,
    expires: i64,
) -> Result<bool, Error> {
    let params = (
        claim.assignment,
        claim.forge,
        claim.external_id,
        claim.run_id,
        claim.owner_id,
        expires,
    );
    match tx.execute(sql::INSERT_LEASE, params) {
        Ok(_) => commit_claim(tx, claim, now),
        Err(error) if sql::is_unique_violation(&error) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn claim_tx(
    conn: &mut Connection,
    claim: &Claim<'_>,
    ttl: Duration,
    available: impl FnOnce(&Connection, i64) -> Result<bool, Error>,
) -> Result<bool, Error> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let now = now_millis();
    let expires = now.saturating_add(duration_millis(ttl));
    if !available(&tx, now)? {
        return Ok(false);
    }
    tx.execute(
        sql::REAP_EXPIRED,
        (claim.assignment, claim.forge, claim.external_id, now),
    )?;
    insert_claim(tx, claim, now, expires)
}
