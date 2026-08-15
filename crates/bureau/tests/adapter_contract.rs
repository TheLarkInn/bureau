//! Strict agent result and adapter-owned usage contract.

use std::collections::BTreeMap;
use std::time::Duration;

use bureau::adapters::{Usage, result_from_agent};
use bureau::contract::{SCHEMA_VERSION, StepOutcome, StepResult, Trust};
use bureau::process::{SpawnOutcome, SpawnResult};

fn result(outcome: StepOutcome, message: &str) -> StepResult {
    StepResult {
        schema: SCHEMA_VERSION.to_owned(),
        outcome,
        outputs: BTreeMap::new(),
        artifacts: Vec::new(),
        trust: Trust::Derived,
        message: message.to_owned(),
    }
}

fn spawn(exit_code: i32, stdout: Vec<u8>) -> SpawnResult {
    SpawnResult {
        outcome: SpawnOutcome::Exited,
        exit_code: Some(exit_code),
        stdout,
        stderr: b"process detail".to_vec(),
        duration: Duration::ZERO,
        error: None,
    }
}

#[test]
fn exit_zero_without_a_contract_result_fails_closed() {
    let spawned = spawn(0, b"ordinary prose".to_vec());
    let result = result_from_agent(&spawned, None, &spawned.stdout);
    assert!(
        result.outcome == StepOutcome::Failure
            && result.message.contains("bureau-io.publish_result"),
        "{result:?}"
    );
}

#[test]
fn published_result_wins_after_successful_exit() {
    let spawned = spawn(0, b"not json".to_vec());
    let result = result_from_agent(
        &spawned,
        Some(result(StepOutcome::NoWork, "published")),
        b"not json",
    );
    assert_eq!(
        (result.outcome, result.message),
        (StepOutcome::NoWork, "published".to_owned())
    );
}

#[test]
fn process_failure_wins_over_a_published_success() {
    let spawned = spawn(7, Vec::new());
    let result = result_from_agent(
        &spawned,
        Some(result(StepOutcome::Success, "published")),
        b"",
    );
    assert_eq!(
        (result.outcome, result.message.as_str()),
        (StepOutcome::Failure, "process detail")
    );
}

#[test]
fn claude_usage_is_measured_from_the_outer_envelope() {
    let bytes =
        br#"{"result":"{}","total_cost_usd":0.125,"usage":{"input_tokens":10,"output_tokens":4}}"#;
    let usage = Usage::from_claude_json(bytes);
    assert_eq!(
        (
            usage.provider.as_str(),
            usage.input_tokens,
            usage.output_tokens,
            usage.cost_usd.map(f64::to_bits),
            usage.cost_basis.as_deref(),
        ),
        (
            "claude",
            Some(10),
            Some(4),
            Some(0.125_f64.to_bits()),
            Some("provider_reported_total_cost_usd"),
        )
    );
}

#[test]
fn malformed_usage_is_explicitly_unknown() {
    let usage = Usage::from_claude_json(b"not-json");
    assert_eq!(
        (usage.provider.as_str(), usage.cost_usd, usage.input_tokens),
        ("claude", None, None)
    );
}

#[test]
fn copilot_otel_usage_sums_tokens_and_ai_credits() {
    let jsonl = br#"{"attributes":[{"key":"gen_ai.usage.input_tokens","value":{"intValue":"10"}},{"key":"gen_ai.usage.total_nano_aiu","value":{"intValue":"1000000000"}}]}
{"gen_ai.usage.output_tokens":4,"total_nano_aiu":500000000}
"#;
    let usage = Usage::from_copilot_otel(jsonl);
    assert_eq!(
        (
            usage.input_tokens,
            usage.output_tokens,
            usage.credits.map(f64::to_bits),
            usage.cost_usd.map(f64::to_bits),
            usage.cost_basis.as_deref(),
        ),
        (
            Some(10),
            Some(4),
            Some(1.5_f64.to_bits()),
            Some(0.015_f64.to_bits()),
            Some("github_ai_credit_at_usd_0.01"),
        )
    );
}
