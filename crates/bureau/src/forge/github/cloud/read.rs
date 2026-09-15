use serde_json::Value;

use super::pages::{Continuation, Endpoint, unique};
use super::{
    AutomationId, Client, Definition, Error, Events, RepositoryRef, Summary, Task, TaskId, request,
};

fn automations_url(repo: &RepositoryRef) -> Result<reqwest::Url, Error> {
    let [owner, name] = repo.segments();
    request::url(&["agents", "repos", owner, name, "automations", "v2"], true)
}

fn task_endpoint(automation: &AutomationId) -> Result<Endpoint, Error> {
    let mut url = request::url(
        &["agents", "automations", automation.as_str(), "tasks"],
        true,
    )?;
    url.query_pairs_mut()
        .append_pair("sort", "created_at")
        .append_pair("direction", "desc")
        .append_pair("is_archived", "false");
    Ok(Endpoint {
        url,
        field: "tasks",
        continuation: Continuation::ShortPage,
    })
}

fn check_definition(
    repo: &RepositoryRef,
    summary: &Summary,
    definition: &Definition,
) -> Result<(), Error> {
    if definition.id != summary.id {
        return Err(Error::Identity(
            "definition ID differs from the selected automation".to_owned(),
        ));
    }
    repo.check(definition.repository.as_ref())?;
    let same_id = summary
        .repository
        .as_ref()
        .zip(definition.repository.as_ref())
        .is_none_or(|(listed, detailed)| listed.id == detailed.id);
    if !same_id {
        return Err(Error::Identity(
            "repository ID changed between listing and detail".to_owned(),
        ));
    }
    Ok(())
}

fn check_events(events: &[Value]) -> Result<(), Error> {
    if events.iter().all(Value::is_object) {
        Ok(())
    } else {
        Err(Error::Response("events must be JSON objects".to_owned()))
    }
}

impl Client {
    /// Lists repo-scoped automations without app adoption state.
    ///
    /// # Errors
    /// Rejects API, pagination, response-shape, or repository-scope failures.
    pub async fn automations(&self, repo: &RepositoryRef) -> Result<Vec<Summary>, Error> {
        let endpoint = Endpoint {
            url: automations_url(repo)?,
            field: "automations",
            continuation: Continuation::Link,
        };
        let collection = self.collect::<Summary>(&endpoint).await?;
        for summary in &collection.items {
            repo.check(summary.repository.as_ref())?;
        }
        unique(collection.items, |summary| summary.id.as_str())
    }

    /// Reads an exact definition only after establishing repo-scoped membership.
    ///
    /// # Errors
    /// Rejects missing membership, changed identities, or failed reads.
    pub async fn definition(
        &self,
        repo: &RepositoryRef,
        id: &AutomationId,
    ) -> Result<Definition, Error> {
        let summaries = self.automations(repo).await?;
        let summary = summaries
            .iter()
            .find(|summary| summary.id == *id)
            .ok_or_else(|| {
                Error::Identity("automation is not in the selected repository inventory".to_owned())
            })?;
        let url = request::url(&["agents", "automations", id.as_str()], true)?;
        let definition = self.json(url).await?;
        check_definition(repo, summary, &definition)?;
        Ok(definition)
    }

    /// Reads task history for a verified repo automation, without inferring dispatch correlation.
    ///
    /// # Errors
    /// Rejects unverifiable task/session identities or incomplete reads.
    pub async fn tasks(
        &self,
        repo: &RepositoryRef,
        automation: &AutomationId,
    ) -> Result<Vec<Task>, Error> {
        self.definition(repo, automation).await?;
        let tasks = self
            .collect::<Task>(&task_endpoint(automation)?)
            .await?
            .items;
        for task in &tasks {
            task.check(automation, &task.id)?;
        }
        unique(tasks, |task| task.id.as_str())
    }

    /// Reads a task by exact selected identity.
    ///
    /// # Errors
    /// Rejects missing automation attribution and task/session identity changes.
    pub async fn task(&self, automation: &AutomationId, task: &TaskId) -> Result<Task, Error> {
        let url = request::url(&["agents", "tasks", task.as_str()], true)?;
        let observed: Task = self.json(url).await?;
        observed.check(automation, task)?;
        Ok(observed)
    }

    /// Reads raw remote events; never synthesizes completion or permission decisions.
    ///
    /// # Errors
    /// Rejects malformed event objects and incomplete pagination.
    pub async fn events(&self, task: &TaskId) -> Result<Events, Error> {
        let url = request::url(&["agents", "tasks", task.as_str(), "events"], true)?;
        let endpoint = Endpoint {
            url,
            field: "events",
            continuation: Continuation::ShortPage,
        };
        let collected = self.collect::<Value>(&endpoint).await?;
        check_events(&collected.items)?;
        Ok(Events {
            events: collected.items,
            reported_total: collected.total,
        })
    }

    /// Submits once; a successful response acknowledges acceptance, not a task identity.
    ///
    /// # Errors
    /// Rejects disabled/event-driven definitions and surfaces all send failures without retry.
    pub async fn dispatch(
        &self,
        repo: &RepositoryRef,
        definition: &Definition,
    ) -> Result<(), Error> {
        repo.check(definition.repository.as_ref())?;
        let event = definition.dispatch_event()?;
        let [owner, name] = repo.segments();
        let url = request::url(
            &[
                "agents",
                "repos",
                owner,
                name,
                "automations",
                definition.id.as_str(),
                "tasks",
            ],
            true,
        )?;
        self.post(url, &serde_json::json!({ "event": event.as_str() }))
            .await
    }
}
