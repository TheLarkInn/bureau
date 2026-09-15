use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bureau::forge::github::cloud::{
    AutomationId, Client, Error, RepositoryRef, Response, TaskId, Transport,
};
use bureau::process::Secret;
use serde_json::{Value, json};

pub const TOKEN: &str = "synthetic-cloud-secret";

pub fn response(status: u16, value: &Value) -> Result<Response, Error> {
    Ok(Response {
        status,
        headers: BTreeMap::new(),
        body: serde_json::to_vec(value).map_err(|error| Error::Response(error.to_string()))?,
    })
}

pub fn linked(value: &Value, target: &str) -> Result<Response, Error> {
    let mut response = response(200, value)?;
    response
        .headers
        .insert("link".to_owned(), format!("<{target}>; rel=\"next\""));
    Ok(response)
}

pub struct Fake {
    replies: Mutex<VecDeque<Result<Response, Error>>>,
    requests: Mutex<Vec<reqwest::Request>>,
}

impl Fake {
    pub fn with_token(replies: Vec<Result<Response, Error>>, token: &str) -> (Client, Arc<Self>) {
        let fake = Arc::new(Self {
            replies: Mutex::new(replies.into()),
            requests: Mutex::new(Vec::new()),
        });
        (
            Client::with_transport(Secret::new(token), fake.clone()),
            fake,
        )
    }

    pub fn client(replies: Vec<Result<Response, Error>>) -> (Client, Arc<Self>) {
        Self::with_token(replies, TOKEN)
    }

    pub fn requests(&self) -> Vec<reqwest::Request> {
        std::mem::take(&mut *self.requests.lock().expect("requests lock"))
    }
}

#[async_trait]
impl Transport for Fake {
    async fn send(&self, request: reqwest::Request) -> Result<Response, Error> {
        self.requests.lock().expect("requests lock").push(request);
        self.replies
            .lock()
            .expect("replies lock")
            .pop_front()
            .expect("unexpected HTTP request")
    }
}

pub fn repo() -> RepositoryRef {
    RepositoryRef::parse("https://github.com/example/project.git").expect("repo fixture")
}

pub fn automation() -> AutomationId {
    AutomationId::try_from("automation-1".to_owned()).expect("automation fixture")
}

pub fn task_id() -> TaskId {
    TaskId::try_from("task-1".to_owned()).expect("task fixture")
}

pub fn summary() -> Value {
    json!({
        "id": "automation-1", "name": "Review", "description": "Review changes",
        "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z",
        "created_by": {}, "repository": {"id": 7, "owner": "example", "name": "project"}
    })
}

pub fn definition() -> Value {
    let mut definition = summary();
    definition["prompt"] = json!("Review the selected repository");
    definition
}

pub fn task() -> Value {
    json!({
        "id": "task-1", "automation_id": "automation-1",
        "state": "waiting_for_user", "status": "provider-defined",
        "created_at": "2026-01-01T00:00:00Z",
        "sessions": [{
            "id": "session-1", "task_id": "task-1", "state": "waiting_for_user",
            "created_at": "2026-01-01T00:00:00Z"
        }]
    })
}

pub fn definition_replies() -> Vec<Result<Response, Error>> {
    vec![
        response(200, &json!({"automations": [summary()]})),
        response(200, &definition()),
    ]
}

pub fn body(request: &reqwest::Request) -> Value {
    let bytes = request
        .body()
        .expect("request body")
        .as_bytes()
        .expect("JSON bytes");
    serde_json::from_slice(bytes).expect("request JSON")
}

pub fn queries(request: &reqwest::Request) -> BTreeMap<String, String> {
    request
        .url()
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

pub fn auth_headers(request: &reqwest::Request) -> bool {
    let headers = request.headers();
    headers
        .get("authorization")
        .is_some_and(|value| value == format!("Bearer {TOKEN}").as_str())
        && headers
            .get("accept")
            .is_some_and(|value| value == "application/json")
        && headers
            .get("user-agent")
            .is_some_and(|value| value == "bureau")
        && !headers.contains_key("copilot-integration-id")
}
