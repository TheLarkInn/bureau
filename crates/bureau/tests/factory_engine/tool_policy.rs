use std::collections::BTreeSet;

use bureau::config::Permission;
use bureau::contract::StepOutcome;

use super::fixture::Fixture;

const READ: [&str; 9] = [
    "builtin:view",
    "builtin:glob",
    "builtin:grep",
    "builtin:task",
    "builtin:read_agent",
    "builtin:list_agents",
    "builtin:write_agent",
    "bureau-io/get_step_context",
    "bureau-io/publish_result",
];
const WRITE: [&str; 8] = [
    "builtin:create",
    "builtin:edit",
    "builtin:str_replace_editor",
    "builtin:apply_patch",
    "builtin:bash",
    "builtin:read_bash",
    "builtin:stop_bash",
    "builtin:list_bash",
];

fn authorized(mode: &str, permission: Permission) -> Fixture {
    let mut fixture = Fixture::create(mode);
    fixture
        .plan
        .roles
        .get_mut("worker")
        .expect("worker role")
        .permissions
        .push(permission);
    fixture
}

fn expected(permission: Permission) -> BTreeSet<String> {
    let mut expected = BTreeSet::from(READ.map(str::to_owned));
    if permission == Permission::RepoWrite {
        expected.extend(WRITE.map(str::to_owned));
    }
    expected
}

fn available(fixture: &Fixture) -> BTreeSet<String> {
    fixture
        .trace()
        .iter()
        .find(|entry| entry["method"] == "session.create")
        .expect("session creation")["params"]["availableTools"]
        .as_array()
        .expect("tool selectors")
        .iter()
        .map(|name| name.as_str().expect("selector").to_owned())
        .collect()
}

#[tokio::test]
async fn read_and_write_grants_use_exact_source_qualified_selectors() {
    for permission in [Permission::RepoRead, Permission::RepoWrite] {
        let fixture = authorized("success", permission);
        let outcome = fixture.engine.run(&fixture.plan).await;
        assert_eq!(
            (outcome.outcome, available(&fixture)),
            (StepOutcome::NoWork, expected(permission)),
            "{outcome:?}"
        );
    }
}

#[tokio::test]
async fn model_unavailability_never_broadens_the_approved_tool_ceiling() {
    let fixture = authorized("unoffered-builtins", Permission::RepoWrite);
    let outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            outcome.outcome,
            fixture.record().can_clean(),
            available(&fixture)
        ),
        (StepOutcome::NoWork, true, expected(Permission::RepoWrite)),
        "{outcome:?}"
    );
}
