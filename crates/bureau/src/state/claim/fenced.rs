//! Keeps lease takeover outside a short, synchronous durable append.

use rusqlite::{Transaction, TransactionBehavior};

use super::super::{Error, now_millis, sql};
use super::LeaseOwner;

fn check(transaction: &Transaction<'_>, owner: &LeaseOwner) -> Result<(), Error> {
    let params = (
        &owner.key.assignment,
        &owner.key.external_id,
        &owner.key.run_id,
        &owner.owner_id,
        now_millis(),
    );
    let is_current = transaction.query_row(sql::OWNED, params, |row| row.get::<_, bool>(0))?;
    if is_current {
        return Ok(());
    }
    Err(Error::LeaseLost(owner.key.run_id.clone()))
}

impl LeaseOwner {
    /// Durable run identity authorized by this ownership generation.
    #[must_use]
    pub fn run_id(&self) -> &str {
        &self.key.run_id
    }

    /// Exact forge identity recorded when this lease owner was created.
    #[must_use]
    pub fn forge(&self) -> &str {
        &self.key.forge
    }

    /// Performs a short durable write while preventing concurrent lease takeover.
    ///
    /// The operation must not await or call back into this store. Acquire any
    /// run-log mutex inside the operation, after the lease lock.
    ///
    /// # Errors
    /// Rejects an expired or replaced owner and propagates database or write failures.
    pub fn with_ownership<T>(
        &self,
        operation: impl FnOnce() -> std::io::Result<T>,
    ) -> Result<T, Error> {
        let mut connection = self.store.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        check(&transaction, self)?;
        let result = operation()?;
        transaction.commit()?;
        drop(connection);
        Ok(result)
    }
}
