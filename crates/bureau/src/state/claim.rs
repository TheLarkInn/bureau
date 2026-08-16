//! Run-owned lease claims, renewal, release, and observation.

use std::sync::Arc;
use std::time::Duration;

use rusqlite::Connection;

use super::{Error, Lease, Store, active_leases, duration_millis, now_millis, sql};

struct Claim<'a> {
    assignment: &'a str,
    forge: &'a str,
    external_id: &'a str,
    run_id: &'a str,
    owner_id: &'a str,
}

/// One supervisor generation's fenced lease identity.
#[derive(Clone)]
pub struct LeaseOwner {
    store: Arc<Store>,
    assignment: String,
    forge: String,
    external_id: String,
    run_id: String,
    owner_id: String,
}

impl LeaseOwner {
    /// Creates a fresh supervisor generation for one durable run.
    /// # Errors
    /// Fails when the operating system random source is unavailable.
    pub fn new(
        store: Arc<Store>,
        assignment: &str,
        forge: &str,
        external_id: &str,
        run_id: &str,
    ) -> Result<Self, Error> {
        Ok(Self {
            store,
            assignment: assignment.to_owned(),
            forge: forge.to_owned(),
            external_id: external_id.to_owned(),
            run_id: run_id.to_owned(),
            owner_id: crate::identity::random_hex()?,
        })
    }

    /// Claims an unowned or expired lease.
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn claim(&self, ttl: Duration) -> Result<bool, Error> {
        self.store.claim_owner(self, ttl)
    }

    /// Renews this generation's still-live lease.
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn renew(&self, ttl: Duration) -> Result<bool, Error> {
        self.store.renew_owner(self, ttl)
    }

    /// Tests whether this generation still owns a live lease.
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn owns(&self) -> Result<bool, Error> {
        self.store.owns(self)
    }

    /// Releases only this supervisor generation.
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn release(&self) -> Result<(), Error> {
        self.store.release_owner(self)
    }

    /// Assignment holding this lease.
    #[must_use]
    pub fn assignment(&self) -> &str {
        &self.assignment
    }

    /// Work item held by this lease.
    #[must_use]
    pub fn external_id(&self) -> &str {
        &self.external_id
    }
}

impl Store {
    /// Attempts to claim an item using its id as the legacy owner id.
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
        self.try_claim_run(assignment, forge, external_id, external_id, ttl)
    }

    /// Attempts to claim an item for one durable run id.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn try_claim_run(
        &self,
        assignment: &str,
        forge: &str,
        external_id: &str,
        run_id: &str,
        ttl: Duration,
    ) -> Result<bool, Error> {
        let now = now_millis();
        let expires = now.saturating_add(duration_millis(ttl));
        let claim = Claim {
            assignment,
            forge,
            external_id,
            run_id,
            owner_id: run_id,
        };
        claim_tx(&mut self.lock(), &claim, now, expires)
    }

    /// Reclaims a crashed run's own lease or claims it after expiry.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn resume_claim(
        &self,
        assignment: &str,
        forge: &str,
        external_id: &str,
        run_id: &str,
        ttl: Duration,
    ) -> Result<bool, Error> {
        self.try_claim_run(assignment, forge, external_id, run_id, ttl)
    }

    /// Releases a legacy claim. Idempotent.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn release(&self, assignment: &str, external_id: &str) -> Result<(), Error> {
        self.release_run(assignment, external_id, external_id)
    }

    /// Releases a claim only when `run_id` owns it.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn release_run(
        &self,
        assignment: &str,
        external_id: &str,
        run_id: &str,
    ) -> Result<(), Error> {
        self.lock()
            .execute(sql::RELEASE, (assignment, external_id, run_id, run_id))?;
        Ok(())
    }

    /// Extends a legacy live lease.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn renew(&self, assignment: &str, external_id: &str, ttl: Duration) -> Result<bool, Error> {
        self.renew_run(assignment, external_id, external_id, ttl)
    }

    /// Extends a live lease only when `run_id` owns it.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn renew_run(
        &self,
        assignment: &str,
        external_id: &str,
        run_id: &str,
        ttl: Duration,
    ) -> Result<bool, Error> {
        let now = now_millis();
        let expires = now.saturating_add(duration_millis(ttl));
        let changed = self.lock().execute(
            sql::RENEW,
            (expires, assignment, external_id, run_id, run_id, now),
        )?;
        Ok(changed > 0)
    }

    /// Live leases for an assignment, excluding expired rows.
    ///
    /// # Errors
    /// Propagates `SQLite` failures.
    pub fn active(&self, assignment: &str) -> Result<Vec<Lease>, Error> {
        active_leases(&self.lock(), assignment)
    }

    fn claim_owner(&self, owner: &LeaseOwner, ttl: Duration) -> Result<bool, Error> {
        let now = now_millis();
        let expires = now.saturating_add(duration_millis(ttl));
        let claim = Claim {
            assignment: &owner.assignment,
            forge: &owner.forge,
            external_id: &owner.external_id,
            run_id: &owner.run_id,
            owner_id: &owner.owner_id,
        };
        claim_tx(&mut self.lock(), &claim, now, expires)
    }

    fn renew_owner(&self, owner: &LeaseOwner, ttl: Duration) -> Result<bool, Error> {
        let now = now_millis();
        let expires = now.saturating_add(duration_millis(ttl));
        let params = (
            expires,
            &owner.assignment,
            &owner.external_id,
            &owner.run_id,
            &owner.owner_id,
            now,
        );
        Ok(self.lock().execute(sql::RENEW, params)? > 0)
    }

    fn owns(&self, owner: &LeaseOwner) -> Result<bool, Error> {
        let params = (
            &owner.assignment,
            &owner.external_id,
            &owner.run_id,
            &owner.owner_id,
            now_millis(),
        );
        Ok(self
            .lock()
            .query_row(sql::OWNED, params, |row| row.get(0))?)
    }

    fn release_owner(&self, owner: &LeaseOwner) -> Result<(), Error> {
        let params = (
            &owner.assignment,
            &owner.external_id,
            &owner.run_id,
            &owner.owner_id,
        );
        self.lock().execute(sql::RELEASE, params)?;
        Ok(())
    }

    pub(super) fn release_terminal(
        &self,
        assignment: &str,
        external_id: &str,
        run_id: &str,
    ) -> Result<(), Error> {
        self.lock()
            .execute(sql::RELEASE_RUN, (assignment, external_id, run_id))?;
        Ok(())
    }
}

fn claim_tx(
    conn: &mut Connection,
    claim: &Claim<'_>,
    now: i64,
    expires: i64,
) -> Result<bool, Error> {
    let tx = conn.transaction()?;
    let key = (claim.assignment, claim.forge, claim.external_id, now);
    tx.execute(sql::REAP_EXPIRED, key)?;
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
