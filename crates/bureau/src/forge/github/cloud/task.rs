use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::automation::User;
use super::{AutomationId, Error, SessionId, TaskId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub task_id: TaskId,
    pub state: String,
    pub created_at: String,
    #[serde(default)]
    pub remote_steerable: bool,
    pub name: Option<String>,
    pub updated_at: Option<String>,
    pub completed_at: Option<String>,
    pub prompt: Option<String>,
    pub head_ref: Option<String>,
    pub base_ref: Option<String>,
    pub model: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub state: String,
    pub created_at: String,
    pub automation_id: Option<AutomationId>,
    pub archived_at: Option<String>,
    pub name: Option<String>,
    pub url: Option<String>,
    pub html_url: Option<String>,
    pub status: Option<String>,
    pub session_count: Option<u32>,
    pub remote_steerable: Option<bool>,
    pub artifacts: Option<Vec<Value>>,
    pub updated_at: Option<String>,
    pub last_updated_at: Option<String>,
    pub creator: Option<User>,
    #[serde(default)]
    pub sessions: Vec<Session>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Task {
    pub(super) fn check(&self, automation: &AutomationId, task: &TaskId) -> Result<(), Error> {
        let matches = self.id == *task
            && self.automation_id.as_ref() == Some(automation)
            && self
                .sessions
                .iter()
                .all(|session| session.task_id == self.id);
        if matches {
            Ok(())
        } else {
            Err(Error::Identity(
                "task, automation, or execution-session identity differs from the selection"
                    .to_owned(),
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Events {
    pub events: Vec<Value>,
    pub reported_total: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub id: u64,
    pub login: String,
}

impl Principal {
    pub(super) fn check(&self, login: &str) -> Result<(), Error> {
        if self.id > 0 && !login.is_empty() && self.login.eq_ignore_ascii_case(login) {
            Ok(())
        } else {
            Err(Error::Identity(
                "credential does not identify the expected GitHub user".to_owned(),
            ))
        }
    }
}
