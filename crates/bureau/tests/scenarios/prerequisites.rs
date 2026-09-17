use std::process::Command;

use bureau::adapters::copilot_factory::definition::Definition;
use bureau::config::{Access, Pipeline, StepDef, StepKind};
use bureau::contract::{StepOutcome, StepResult};

use super::{config, root};

fn run_guard(guard: &StepDef) -> StepResult {
    let output = Command::new("sh")
        .args(["-c", guard.run.as_deref().expect("guard command")])
        .env_clear()
        .output()
        .expect("offline guard");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    StepResult::from_json(&output.stdout).expect("guard result")
}

fn node_output(args: &[&str], file: &str) -> std::process::Output {
    Command::new("node")
        .args(args)
        .arg(root().join("local-sdk-factory/fixtures").join(file))
        .output()
        .expect("offline Node fixture")
}

#[test]
fn cloud_setup_only_allows_inspection_and_has_no_daemon_work() {
    let config = config("cloud-automation");
    assert_eq!(
        (
            config.repos["code"].access,
            config.assignments.len(),
            config.pipelines.len(),
            config.label_rules.len(),
        ),
        (Access::Read, 0, 0, 0)
    );
}

#[test]
fn unqualified_factory_guard_reports_blocked_not_success() {
    let config = config("local-sdk-factory");
    let guard = &config.pipelines["local-sdk-factory"].steps[0];
    let result = run_guard(guard);
    assert_eq!(
        (
            guard.kind,
            guard.copilot_factory.is_none(),
            result.outcome,
            guard.on_blocked.as_deref()
        ),
        (
            StepKind::Deterministic,
            true,
            StepOutcome::Blocked,
            Some("escalate")
        )
    );
}

#[test]
fn incomplete_factory_example_rejects_both_placeholder_digests() {
    let mut config = config("local-sdk-factory");
    let text =
        std::fs::read_to_string(root().join("local-sdk-factory/qualified-pipeline.yaml.example"))
            .expect("qualified template");
    let pipeline: Pipeline = serde_yaml_ng::from_str(&text).expect("pipeline shape");
    config.pipelines.insert(pipeline.name.clone(), pipeline);
    let errors = bureau::config::validate(&config);
    let fields = [
        "copilot_factory.extension_digest",
        "copilot_factory.runtime.digest",
    ];
    for field in fields {
        assert!(
            errors.iter().any(|error| error.message.contains(field)),
            "{errors:?}"
        );
    }
}

#[test]
fn concrete_provider_metadata_uses_the_real_factory_argument_validator() {
    let bytes =
        std::fs::read(root().join("local-sdk-factory/provider/factory.json")).expect("metadata");
    let cases = [
        (serde_json::Value::Null, true),
        (serde_json::json!({"focus": "Check the contract."}), true),
        (serde_json::json!({"focus": ""}), false),
        (serde_json::json!({"extra": true}), false),
        (serde_json::json!([]), false),
    ];
    for (args, accepted) in cases {
        assert_eq!(Definition::parse(&bytes, "review", &args).is_ok(), accepted);
    }
}

#[test]
fn concrete_provider_returns_a_real_bureau_result_shape_offline() {
    let output = node_output(&[], "provider-result.mjs");
    let result = StepResult::from_json(&output.stdout).expect("complete v2 result");
    assert_eq!(
        (output.status.code(), result.outcome, result.trust),
        (
            Some(0),
            StepOutcome::Success,
            bureau::contract::Trust::Derived
        )
    );
}

#[test]
fn provider_child_failures_and_cancellation_are_covered_offline() {
    let output = node_output(&["--test", "--test-reporter=tap"], "provider.test.mjs");
    let text = String::from_utf8(output.stdout).expect("TAP");
    assert_eq!(
        (
            output.status.code(),
            text.contains("\nnot ok "),
            text.contains("\nok 1 ")
        ),
        (Some(0), false, true),
        "{text}"
    );
}
