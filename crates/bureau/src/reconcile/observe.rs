//! Forge and budget observation before any claims are created.

use std::sync::Arc;

use super::{Error, Reconciler, forge_key};
use crate::config::Assignment;
use crate::forge::{Forge, Item, Pr};
use crate::state::Lease;

pub(super) struct Observed<'a> {
    pub(super) assignment: &'a Assignment,
    pub(super) forge: Arc<dyn Forge>,
    pub(super) desired: Vec<Item>,
    pub(super) open_prs: Vec<Pr>,
    pub(super) inflight: Vec<Lease>,
    pub(super) headroom: usize,
}

fn bad_assignment(name: &str, reason: &str) -> Error {
    let parse = crate::forge::Error::Parse(format!("assignment `{name}`: {reason}"));
    Error::Forge(parse)
}

impl Reconciler {
    pub(super) async fn observe_all(&self) -> (Vec<Observed<'_>>, Vec<Error>) {
        let (mut observed, mut failed) = (Vec::new(), Vec::new());
        for (name, assignment) in &self.config.assignments {
            match self.observe(name, assignment).await {
                Ok(assignment) => observed.push(assignment),
                Err(error) => failed.push(error),
            }
        }
        (observed, failed)
    }

    /// The primary repo's registry URL: forge calls take a URL, not the
    /// registry name (DESIGN.md section 8's pseudocode elides the lookup).
    fn primary_url(&self, name: &str, assignment: &Assignment) -> Result<&str, Error> {
        let repo = assignment
            .primary_repo()
            .ok_or_else(|| bad_assignment(name, "lists no repos"))?;
        self.config
            .repos
            .get(repo)
            .map(|repo| repo.url.as_str())
            .ok_or_else(|| bad_assignment(name, "primary repo is not in the registry"))
    }

    async fn observe<'a>(
        &'a self,
        name: &str,
        assignment: &'a Assignment,
    ) -> Result<Observed<'a>, Error> {
        let forge = self.work_forge(name, assignment)?;
        let url = self.primary_url(name, assignment)?;
        let desired = Self::desired(forge, assignment).await?;
        let open_prs = forge.open_prs(url, &assignment.branch_prefix).await?;
        let inflight = self.state.active(name)?;
        let headroom = self
            .state
            .headroom(name, &assignment.limits, open_prs.len())?;
        Ok(Observed {
            assignment,
            forge: forge.clone(),
            desired,
            open_prs,
            inflight,
            headroom,
        })
    }

    fn work_forge(&self, name: &str, assignment: &Assignment) -> Result<&Arc<dyn Forge>, Error> {
        self.forges.get(name).ok_or_else(|| {
            bad_assignment(
                name,
                &format!("forge `{}` has no client", forge_key(assignment.work.forge)),
            )
        })
    }

    async fn desired(forge: &Arc<dyn Forge>, assignment: &Assignment) -> Result<Vec<Item>, Error> {
        let items = forge
            .query(&assignment.work.source, &assignment.work.filter)
            .await?;
        Ok(items
            .into_iter()
            .filter_map(|item| super::approved_item(assignment, item))
            .collect())
    }
}
