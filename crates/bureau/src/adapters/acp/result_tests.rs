use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, Cost, SessionNotification, SessionUpdate, TextContent, UsageUpdate,
};

use super::{events, setup};
use crate::config::{AdapterKind, Role};
use crate::contract::{SCHEMA_VERSION, StepOutcome, StepRequest, StepResult, Trust, WorkItem};
use crate::mcp::Session;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    session: Session,
    request: StepRequest,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "bureau-acp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("worktree");
        let request = StepRequest {
            schema: SCHEMA_VERSION.to_owned(),
            run_id: "run".to_owned(),
            step: "step".to_owned(),
            worktree: root.clone(),
            item: WorkItem::default(),
            trust: Trust::Maintainer,
            inputs: BTreeMap::new(),
            artifacts: BTreeMap::new(),
        };
        Self {
            session: Session::create(&request).expect("session"),
            request,
            root,
        }
    }

    fn publish(&self, outcome: StepOutcome) {
        let result = StepResult {
            schema: SCHEMA_VERSION.to_owned(),
            outcome,
            outputs: BTreeMap::new(),
            artifacts: Vec::new(),
            trust: Trust::Derived,
            message: "published".to_owned(),
        };
        std::fs::write(
            self.session.result_path(),
            result.to_json().expect("result"),
        )
        .expect("publish");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).expect("cleanup");
    }
}

fn state() -> events::SharedEvents {
    let state = events::Events::new("claude", &[], None, None);
    events::lock(&state).expect("events").session = Some("session".into());
    state
}

fn update(state: &events::SharedEvents, update: SessionUpdate) {
    events::lock(state)
        .expect("events")
        .update(SessionNotification::new("session", update))
        .expect("update");
}

fn role() -> Role {
    Role {
        name: "worker".to_owned(),
        agent: "/bureau:worker".to_owned(),
        adapter: AdapterKind::Copilot,
        permissions: vec![],
        min_trust: Trust::Maintainer,
    }
}

#[test]
fn acp_mcp_configuration_uses_absolute_executable_and_explicit_context() {
    let fixture = Fixture::new();
    let request =
        setup::new_session(&role(), &fixture.request, &fixture.session, "worker").expect("setup");
    let json = serde_json::to_value(&request).expect("JSON");
    let server = &json["mcpServers"][0];
    let actual = (
        server["name"].as_str(),
        server["args"] == serde_json::json!(["mcp", "serve"]),
        server["env"].as_array().map(Vec::len),
        request.cwd == fixture.root,
        request.meta.is_none(),
        PathBuf::from(server["command"].as_str().expect("command")).is_absolute(),
    );
    assert_eq!(actual, (Some("bureau-io"), true, Some(2), true, true, true));
}

#[test]
fn successful_turn_preserves_every_published_domain_outcome() {
    for outcome in [
        StepOutcome::Success,
        StepOutcome::Failure,
        StepOutcome::Blocked,
        StepOutcome::NoWork,
    ] {
        let fixture = Fixture::new();
        fixture.publish(outcome);
        assert_eq!(
            super::finish(&fixture.session, &state(), Ok(()), &[])
                .result
                .outcome,
            outcome
        );
    }
}

#[test]
fn failed_turn_cannot_be_rescued_by_published_success() {
    let fixture = Fixture::new();
    fixture.publish(StepOutcome::Success);
    let execution = super::finish(&fixture.session, &state(), Err("cancelled".to_owned()), &[]);
    assert_eq!(
        (execution.result.outcome, execution.result.message.as_str()),
        (StepOutcome::Failure, "cancelled")
    );
}

#[test]
fn process_capture_errors_cannot_be_rescued_by_clean_protocol_completion() {
    let spawned = crate::process::SpawnResult {
        outcome: crate::process::SpawnOutcome::Exited,
        exit_code: Some(0),
        stdout: vec![],
        stderr: vec![],
        duration: std::time::Duration::ZERO,
        error: Some("stderr capture failed".to_owned()),
    };
    assert!(super::completed(&spawned, Some(Ok(())), &[]).is_err());
}

#[test]
fn notification_failure_overrides_a_clean_turn_and_published_success() {
    let fixture = Fixture::new();
    fixture.publish(StepOutcome::Success);
    let state = state();
    {
        let mut events = events::lock(&state).expect("events");
        events.agent = Some("worker".to_owned());
        let changed = agent_client_protocol::schema::v1::ConfigOptionUpdate::new(
            super::test_peer::options("default"),
        );
        events.receive(SessionNotification::new(
            "session",
            SessionUpdate::ConfigOptionUpdate(changed),
        ));
    }
    let execution = super::finish(&fixture.session, &state, Ok(()), &[]);
    assert_eq!(execution.result.outcome, StepOutcome::Failure);
}

#[test]
fn prose_and_invalid_publication_fail_closed() {
    let fixture = Fixture::new();
    let first = super::finish(&fixture.session, &state(), Ok(()), &[])
        .result
        .outcome;
    std::fs::write(fixture.session.result_path(), br#"{"schema":"v1"}"#).expect("invalid result");
    let second = super::finish(&fixture.session, &state(), Ok(()), &[])
        .result
        .outcome;
    assert_eq!(
        (first, second),
        (StepOutcome::Failure, StepOutcome::Failure)
    );
}

#[test]
fn only_agent_message_text_can_supply_a_fallback_result() {
    let fixture = Fixture::new();
    fixture.publish(StepOutcome::NoWork);
    let json = std::fs::read_to_string(fixture.session.result_path()).expect("published");
    std::fs::remove_file(fixture.session.result_path()).expect("remove publication");
    let state = state();
    update(&state, SessionUpdate::UserMessageChunk(text(json.clone())));
    let ignored = captured(&state);
    let agent = self::state();
    update(&agent, SessionUpdate::AgentMessageChunk(text(json)));
    assert_eq!(
        (
            ignored.is_empty(),
            super::finish(&fixture.session, &agent, Ok(()), &[])
                .result
                .outcome
        ),
        (true, StepOutcome::NoWork)
    );
}

fn text(text: String) -> ContentChunk {
    ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
}

fn captured(state: &events::SharedEvents) -> Vec<u8> {
    events::lock(state)
        .expect("events")
        .finish()
        .expect("capture")
}

#[tokio::test]
async fn structured_peer_message_survives_the_official_transport_and_domain_decoder() {
    let fixture = Fixture::new();
    let (completion, response, _) =
        super::test_peer::run(super::test_peer::Case::StructuredResult).await;
    completion.expect("ACP exchange");
    let result = super::domain_result(&fixture.session, &response).expect("domain result");
    assert_eq!(result.outcome, StepOutcome::NoWork);
}

#[test]
fn absent_invalid_or_non_usd_cost_is_unknown() {
    for cost in [
        None,
        Some(Cost::new(-1.0, "USD")),
        Some(Cost::new(2.0, "EUR")),
        Some(Cost::new(f64::NAN, "USD")),
    ] {
        let state = state();
        update(
            &state,
            SessionUpdate::UsageUpdate(UsageUpdate::new(40, 100).cost(cost)),
        );
        assert_eq!(events::lock(&state).expect("events").usage.cost_usd, None);
    }
}
