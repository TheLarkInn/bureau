use bureau::config::{AdapterKind, Config, CopilotFactory, StepDef, StepKind};
use serde_json::{Value, json};

use super::{agent, config, errors, factory};

const LEGACY_STEP: &str = r#"{
    "name":"review","type":"agent","role":"reviewer","run":null,
    "fixture":null,"trust":null,"over":null,"on":{},"steps":[],
    "completion":null,"max_concurrent":null,"next":"done",
    "on_failure":null,"on_blocked":null,"on_no_work":null,
    "inputs_from":[],"max_attempts":1,"timeout_secs":null
}"#;

const INVALID_FIELDS: [&str; 9] = [
    "max_attempts",
    "copilot_factory.name",
    "copilot_factory.extension",
    "copilot_factory.args",
    "max_concurrent_subagents",
    "max_total_subagents",
    "timeout_seconds",
    "max_ai_credits",
    "unknown role `missing`",
];

fn with_previous_input(selection: CopilotFactory) -> Config {
    let previous: StepDef =
        serde_yaml_ng::from_str("name: prepare\ntype: deterministic\nrun: 'true'\nnext: review")
            .expect("earlier step");
    let mut step = agent(Some(selection));
    step.inputs_from = vec!["prepare".to_owned()];
    config(vec![previous, step], AdapterKind::Copilot)
}

fn concurrent(selection: Option<CopilotFactory>) -> Config {
    let group: StepDef = serde_yaml_ng::from_str(
        "name: evidence\ntype: concurrent\nsteps: [check, review]\nnext: done",
    )
    .expect("group");
    let check: StepDef =
        serde_yaml_ng::from_str("name: check\ntype: deterministic\nrun: 'true'").expect("member");
    let mut review = agent(selection);
    review.next = None;
    config(vec![group, check, review], AdapterKind::Copilot)
}

#[test]
fn copilot_agent_steps_allow_explicit_local_factories() {
    let config = config(vec![agent(Some(factory("")))], AdapterKind::Copilot);
    assert!(errors(&config).is_empty());
    assert_eq!(config.roles["reviewer"].agent, "/bureau:reviewer");
}

#[test]
fn the_resolved_role_must_use_the_copilot_adapter() {
    for adapter in [AdapterKind::Fake, AdapterKind::Claude] {
        let config = config(vec![agent(Some(factory("")))], adapter);
        let found = errors(&config);
        assert!(
            found[0].contains("`copilot_factory` requires a role with the `copilot` adapter"),
            "{found:?}"
        );
    }
}

#[test]
fn factories_are_rejected_on_every_other_step_kind() {
    for kind in [
        StepKind::Deterministic,
        StepKind::Decision,
        StepKind::Concurrent,
    ] {
        let mut step = agent(Some(factory("")));
        step.kind = kind;
        let expected = format!("`copilot_factory` does not apply to {} steps", kind.name());
        assert!(step.field_errors().contains(&expected), "{kind:?}");
    }
}

#[test]
fn unknown_roles_do_not_hide_other_factory_or_step_errors() {
    let mut selection = factory(
        "args: []\nlimits: {max_concurrent_subagents: 0, max_total_subagents: 0, timeout_seconds: 0, max_ai_credits: -1}",
    );
    selection.name.clear();
    selection.extension = "user:review".to_owned();
    let mut step = agent(Some(selection));
    step.role = Some("missing".to_owned());
    step.max_attempts = 0;
    let found = errors(&config(vec![step], AdapterKind::Copilot));
    let reported = INVALID_FIELDS.map(|field| found.iter().any(|error| error.contains(field)));
    assert_eq!((found.len(), reported), (9, [true; 9]), "{found:?}");
    assert!(
        found
            .iter()
            .all(|error| error.contains("pipeline `inspect` step `review`"))
    );
}

#[test]
fn concurrent_factory_members_report_the_resume_ownership_limitation() {
    let found = errors(&concurrent(Some(factory(""))));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("not supported on concurrent evidence members")
            && found[0].contains("member worktree/runtime ownership across factory resume"),
        "{found:?}"
    );
}

#[test]
fn nonfactory_concurrent_members_are_unchanged() {
    let config = concurrent(None);
    let encoded = serde_json::to_value(&config).expect("encode concurrent config");
    assert!(errors(&config).is_empty());
    assert!(
        encoded["pipelines"]["inspect"]["steps"]
            .as_array()
            .expect("steps")
            .iter()
            .all(|step| step.get("copilot_factory").is_none())
    );
}

#[test]
fn old_steps_preserve_canonical_serialization_and_behavior() {
    let expected: Value = serde_json::from_str(LEGACY_STEP).expect("legacy JSON");
    let step: StepDef = serde_json::from_value(expected.clone()).expect("legacy step");
    assert_eq!(
        (
            step.copilot_factory.clone(),
            serde_json::to_value(&step).expect("encode")
        ),
        (None, expected)
    );
    for adapter in [AdapterKind::Copilot, AdapterKind::Claude, AdapterKind::Fake] {
        assert!(errors(&config(vec![step.clone()], adapter)).is_empty());
    }
}

#[test]
fn inputs_from_does_not_inject_or_interpolate_factory_args() {
    for args in [
        Value::Null,
        json!({}),
        json!({"literal": "${inputs_from.prepare}"}),
    ] {
        let mut selection = factory("");
        selection.args = args.clone();
        let config = with_previous_input(selection);
        let bytes = serde_json::to_vec(&config).expect("encode config");
        let restored: Config = serde_json::from_slice(&bytes).expect("restore config");
        let step = &restored.pipelines["inspect"].steps[1];
        assert_eq!(
            (
                &step.copilot_factory.as_ref().expect("factory").args,
                errors(&restored)
            ),
            (&args, Vec::<String>::new())
        );
    }
}
