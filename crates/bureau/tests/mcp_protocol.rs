//! Offline MCP protocol and session tests.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use bureau::contract::{
    Artifact, SCHEMA_VERSION, StepOutcome, StepRequest, StepResult, Trust, WorkItem,
};
use bureau::mcp::{Session, serve};
use serde_json::{Value, json};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "bureau-mcp-test-{}-{}-{tag}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("create test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn work_item() -> WorkItem {
    WorkItem {
        external_id: "acme/web#42".to_owned(),
        title: "Fix the flaky login test".to_owned(),
        body: "The login test fails intermittently on CI.".to_owned(),
        url: "https://example.invalid/acme/web/issues/42".to_owned(),
        labels: vec!["bug".to_owned(), "agent-eligible".to_owned()],
    }
}

fn request(worktree: &Path) -> StepRequest {
    StepRequest {
        schema: SCHEMA_VERSION.to_owned(),
        run_id: "run-42".to_owned(),
        step: "implement".to_owned(),
        worktree: worktree.to_path_buf(),
        item: work_item(),
        trust: Trust::Maintainer,
        inputs: BTreeMap::from([("issue".to_owned(), json!(42))]),
        artifacts: BTreeMap::from([("log".to_owned(), PathBuf::from("test.log"))]),
    }
}

fn exchange(session: &Session, messages: &[Value]) -> Vec<Value> {
    let mut input = messages
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .expect("serialize messages")
        .join("\n");
    input.push('\n');
    exchange_text(session, &input)
}

fn exchange_text(session: &Session, input: &str) -> Vec<Value> {
    let mut output = Vec::new();
    serve(session.paths(), Cursor::new(input.as_bytes()), &mut output).expect("serve messages");
    String::from_utf8(output)
        .expect("utf8 output")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON-RPC response"))
        .collect()
}

fn tool_call(id: u64, name: &str, arguments: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": name, "arguments": arguments}
    })
}

#[test]
fn handshake_ping_and_tool_list() {
    let dir = TestDir::new("handshake");
    let session = Session::create(&request(dir.path())).expect("create session");
    let replies = exchange(&session, &handshake_messages());
    assert_eq!(
        handshake_summary(&replies),
        json!({
            "ids":[1, 2, 3], "server":"bureau-io", "ping":{},
            "tools":["get_step_context", "publish_result"]
        })
    );
}

fn handshake_messages() -> [Value; 4] {
    [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","clientInfo":{"name":"test","version":"1"}
        }}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"ping"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}),
    ]
}

fn handshake_summary(replies: &[Value]) -> Value {
    json!({
        "ids": response_ids(replies),
        "server": replies[0]["result"]["serverInfo"]["name"],
        "ping": replies[1]["result"],
        "tools": tool_names(&replies[2])
    })
}

fn response_ids(replies: &[Value]) -> Vec<Option<u64>> {
    replies.iter().map(|reply| reply["id"].as_u64()).collect()
}

fn tool_names(reply: &Value) -> Vec<&str> {
    reply["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect()
}

#[test]
fn context_returns_the_exact_validated_request() {
    let dir = TestDir::new("context");
    let expected = request(dir.path());
    let session = Session::create(&expected).expect("create session");
    let replies = exchange(&session, &[tool_call(1, "get_step_context", &json!({}))]);
    let text = replies[0]["result"]["content"][0]["text"]
        .as_str()
        .expect("context text");
    let actual: StepRequest = serde_json::from_str(text).expect("context request");
    assert_eq!(actual, expected);
}

#[test]
fn context_hands_the_agent_its_work_item() {
    let dir = TestDir::new("context-item");
    let session = Session::create(&request(dir.path())).expect("create session");
    let replies = exchange(&session, &[tool_call(1, "get_step_context", &json!({}))]);
    let text = replies[0]["result"]["content"][0]["text"]
        .as_str()
        .expect("context text");
    let actual: StepRequest = serde_json::from_str(text).expect("context request");
    assert_eq!(actual.item, work_item());
}

#[test]
fn publish_is_one_shot_and_cannot_overwrite() {
    let dir = TestDir::new("publish-once");
    let session = Session::create(&request(dir.path())).expect("create session");
    let first = json!({"outcome":"no-work","outputs":{"answer":42},"artifacts":[{
        "name":"report","path":"report.txt"
    }]});
    let second = json!({"outcome":"failure","message":"replace first result"});
    let replies = exchange(
        &session,
        &[
            tool_call(1, "publish_result", &first),
            tool_call(2, "publish_result", &second),
        ],
    );
    let published = session.published().expect("read result").expect("result");
    let expected = rich_result();
    let flags = result_error_flags(&replies);
    assert_eq!((flags, published), (vec![false, true], expected));
}

fn rich_result() -> StepResult {
    StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome: StepOutcome::NoWork,
        outputs: BTreeMap::from([("answer".to_owned(), json!(42))]),
        artifacts: vec![Artifact {
            name: "report".to_owned(),
            path: PathBuf::from("report.txt"),
        }],
        trust: Trust::Derived,
        message: String::new(),
    }
}

fn result_error_flags(replies: &[Value]) -> Vec<bool> {
    replies
        .iter()
        .map(|reply| reply["result"]["isError"].as_bool().expect("isError"))
        .collect()
}

#[test]
fn publish_defaults_and_server_owned_fields() {
    let dir = TestDir::new("publish-defaults");
    let session = Session::create(&request(dir.path())).expect("create session");
    exchange(
        &session,
        &[tool_call(
            1,
            "publish_result",
            &json!({"outcome":"success"}),
        )],
    );
    let result = session.published().expect("read result").expect("result");
    assert_eq!(result, default_result());
}

fn default_result() -> StepResult {
    StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome: StepOutcome::Success,
        outputs: BTreeMap::new(),
        artifacts: Vec::new(),
        trust: Trust::Derived,
        message: String::new(),
    }
}

#[test]
fn publish_rejects_agent_owned_protocol_fields() {
    let dir = TestDir::new("publish-owned");
    let session = Session::create(&request(dir.path())).expect("create session");
    let messages = ["schema", "trust", "cost_usd"]
        .into_iter()
        .enumerate()
        .map(|(id, field)| {
            let mut arguments = json!({"outcome":"success"});
            arguments[field] = json!("agent-value");
            tool_call(
                u64::try_from(id).expect("message id"),
                "publish_result",
                &arguments,
            )
        })
        .collect::<Vec<_>>();
    let replies = exchange(&session, &messages);
    let codes = replies
        .iter()
        .map(|reply| reply["error"]["code"].as_i64())
        .collect::<Vec<_>>();
    assert_eq!(
        (codes, session.published().expect("published state")),
        (vec![Some(-32602); 3], None)
    );
}

#[test]
fn malformed_and_unknown_requests_return_errors() {
    let dir = TestDir::new("errors");
    let session = Session::create(&request(dir.path())).expect("create session");
    let input = concat!(
        "not json\n",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"unknown\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",",
        "\"params\":{\"name\":\"unknown\",\"arguments\":{}}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":[]}\n"
    );
    let replies = exchange_text(&session, input);
    let codes = replies
        .iter()
        .map(|reply| reply["error"]["code"].as_i64())
        .collect::<Vec<_>>();
    assert_eq!(
        codes,
        vec![Some(-32700), Some(-32601), Some(-32602), Some(-32602)]
    );
}
