use rusqlite::{Connection, Transaction, TransactionBehavior};

use super::super::{Error, sql};

#[cfg(test)]
mod tests;

pub(super) struct Claim<'a> {
    pub(super) assignment: &'a str,
    pub(super) forge: &'a str,
    pub(super) external_id: &'a str,
    pub(super) run_id: &'a str,
    pub(super) owner_id: &'a str,
}

fn insert_claim(tx: Transaction<'_>, claim: &Claim<'_>, expires: i64) -> Result<bool, Error> {
    let params = (
        claim.assignment,
        claim.forge,
        claim.external_id,
        claim.run_id,
        claim.owner_id,
        expires,
    );
    match tx.execute(sql::INSERT_LEASE, params) {
        Ok(_) => {
            tx.commit()?;
            Ok(true)
        }
        Err(error) if sql::is_unique_violation(&error) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn claim_tx(
    conn: &mut Connection,
    claim: &Claim<'_>,
    now: i64,
    expires: i64,
    available: impl FnOnce(&Connection, i64) -> Result<bool, Error>,
) -> Result<bool, Error> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !available(&tx, now)? {
        return Ok(false);
    }
    tx.execute(
        sql::REAP_EXPIRED,
        (claim.assignment, claim.forge, claim.external_id, now),
    )?;
    insert_claim(tx, claim, expires)
}
