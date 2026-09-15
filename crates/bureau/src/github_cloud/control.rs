use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::forge::github::cloud::{AutomationId, Client, Definition};
use crate::process::Secret;
use crate::state::{LeaseOwner, Store, maintain_lease};

use super::{
    Error, LEASE_ASSIGNMENT, Log, Selection, Start, State, lease_key, read_state, validate_key,
};

const LEASE_TTL: Duration = Duration::from_secs(90);

pub struct Control<'a> {
    pub client: &'a Client,
    pub selection: &'a Selection,
    pub store: Arc<Store>,
    pub root: &'a Path,
    pub secrets: &'a [Secret],
}

impl Control<'_> {
    pub(super) fn owner(&self, key: &str) -> Result<LeaseOwner, Error> {
        validate_key(key)?;
        self.selection.check_client(self.client)?;
        let external_id = lease_key(&self.selection.scope().repo, key);
        let owner = LeaseOwner::new(
            self.store.clone(),
            LEASE_ASSIGNMENT,
            "github_cloud",
            &external_id,
            key,
        )?;
        if !owner.claim(LEASE_TTL)? {
            return Err(Error::Busy(key.to_owned()));
        }
        Ok(owner)
    }

    pub(super) fn existing(
        &self,
        key: &str,
        automation: &AutomationId,
    ) -> Result<Option<State>, Error> {
        if !self.root.join(key).try_exists()? {
            return Ok(None);
        }
        let state = read_state(self.root, key)?;
        self.selection.check_binding(&state, automation)?;
        Ok(Some(state))
    }

    pub(super) fn create(
        &self,
        key: &str,
        definition: &Definition,
        owner: &LeaseOwner,
    ) -> Result<Log, Error> {
        let start = Start {
            request_id: key.to_owned(),
            scope: self.selection.scope().clone(),
            automation_id: definition.id.as_str().to_owned(),
            definition: serde_json::to_value(definition)?,
        };
        Ok(Log::create(self.root, start, &self.log_secrets(), owner)?)
    }

    pub(super) fn open(&self, key: &str, owner: &LeaseOwner) -> Result<Log, Error> {
        Ok(Log::open(self.root, key, &self.log_secrets(), owner)?)
    }

    fn log_secrets(&self) -> Vec<Secret> {
        let mut secrets = self.secrets.to_vec();
        secrets.push(self.client.credential());
        secrets
    }
}

pub(super) fn require_owner(owner: &LeaseOwner) -> Result<(), Error> {
    if owner.owns()? {
        Ok(())
    } else {
        Err(Error::LeaseLost)
    }
}

fn release(owner: &LeaseOwner, result: Result<State, Error>) -> Result<State, Error> {
    match (owner.release(), result) {
        (Ok(()), result) => result,
        (Err(error), Ok(_)) => Err(Error::Release(error.to_string())),
        (Err(error), Err(operation)) => {
            Err(Error::Release(format!("{error}; operation: {operation}")))
        }
    }
}

pub(super) async fn supervised(
    control: &Control<'_>,
    key: &str,
    owner: &LeaseOwner,
    future: impl Future<Output = Result<State, Error>>,
) -> Result<State, Error> {
    let cancel = control.root.join(key).join("LOCAL_OPERATION_STOPPED");
    let result = maintain_lease(owner.clone(), LEASE_TTL, &cancel, future).await;
    release(owner, result.unwrap_or(Err(Error::LeaseLost)))
}
