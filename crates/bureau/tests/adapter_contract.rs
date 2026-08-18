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

/// One contract document, as an agent CLI emits it.
fn document(message: &str) -> String {
    serde_json::to_string(&result(StepOutcome::Success, message)).expect("serialize document")
}

#[test]
fn a_transcript_wrapped_result_is_recovered_not_reported_missing() {
    // What the copilot CLI actually writes: tool transcript on stdout,
    // then the contract document (issue #23). The agent published a
    // valid result; reporting it as missing discards finished work.
    let stdout = format!(
        "● Read label.rs\n  └ 61 lines read\n\n{}\n",
        document("implemented")
    )
    .into_bytes();
    let spawned = spawn(0, stdout);
    let result = result_from_agent(&spawned, None, &spawned.stdout);
    assert_eq!(
        (result.outcome, result.message.as_str()),
        (StepOutcome::Success, "implemented")
    );
}

#[test]
fn the_last_document_wins_when_output_holds_several() {
    // An agent may quote an example before answering; the answer is last.
    let stdout = format!(
        "example: {}\nfinal: {}\n",
        document("example"),
        document("final")
    )
    .into_bytes();
    let spawned = spawn(0, stdout);
    assert_eq!(
        result_from_agent(&spawned, None, &spawned.stdout).message,
        "final"
    );
}

#[test]
fn output_without_a_document_still_fails_closed() {
    let cases: [&[u8]; 3] = [b"ordinary prose", b"", br#"{"schema":"v7"}"#];
    for bytes in cases {
        let spawned = spawn(0, bytes.to_vec());
        let result = result_from_agent(&spawned, None, &spawned.stdout);
        assert_eq!(
            result.outcome,
            StepOutcome::Failure,
            "{:?}",
            String::from_utf8_lossy(bytes)
        );
    }
}

#[test]
fn a_large_document_inside_the_searched_tail_is_still_recovered() {
    // Pins the positive side of the bound: a real answer ends the
    // buffer, so a big one must still be found. Without this, shrinking
    // the window would break no test.
    let big = "x".repeat(200_000);
    let stdout = format!("● Read big.rs\n\n{}\n", document(&big)).into_bytes();
    let spawned = spawn(0, stdout);
    let result = result_from_agent(&spawned, None, &spawned.stdout);
    assert_eq!(
        (result.outcome, result.message.len()),
        (StepOutcome::Success, big.len())
    );
}

#[test]
fn a_document_far_past_the_searched_tail_is_not_recovered() {
    // The scan is bounded so output holding no document cannot cost a
    // parse attempt at every brace of a multi-megabyte transcript. A
    // real answer is the CLI's last output, so it is inside the window.
    let stdout = format!("{}{}", document("buried"), "{ noise\n".repeat(80_000)).into_bytes();
    let spawned = spawn(0, stdout);
    let result = result_from_agent(&spawned, None, &spawned.stdout);
    assert_eq!(result.outcome, StepOutcome::Failure);
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
