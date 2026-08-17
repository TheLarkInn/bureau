//! Adapter-measured model usage.

use serde::{Deserialize, Serialize};

use crate::contract::StepResult;

fn json(bytes: &[u8]) -> Option<serde_json::Value> {
    serde_json::from_slice(bytes).ok()
}

fn integer(value: Option<&serde_json::Value>, key: &str) -> Option<u64> {
    value?.get(key)?.as_u64()
}

fn number(value: Option<&serde_json::Value>, key: &str) -> Option<f64> {
    value?
        .get(key)?
        .as_f64()
        .filter(|number| number.is_finite() && *number >= 0.0)
}

fn unsigned(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(number) => number.as_u64(),
        serde_json::Value::String(number) => number.parse().ok(),
        serde_json::Value::Object(map) => ["intValue", "doubleValue"]
            .into_iter()
            .find_map(|key| map.get(key).and_then(unsigned)),
        _ => None,
    }
}

fn numeric(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(number) => number.parse().ok(),
        serde_json::Value::Object(map) => ["intValue", "doubleValue"]
            .into_iter()
            .find_map(|key| map.get(key).and_then(numeric)),
        _ => None,
    }
    .filter(|number| number.is_finite() && *number >= 0.0)
}

fn add_count(value: &serde_json::Value, total: &mut u64, seen: &mut bool) {
    if let Some(value) = unsigned(value) {
        *total = total.saturating_add(value);
        *seen = true;
    }
}

/// Provider usage measured outside agent-controlled result data.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    /// Adapter/provider that measured the usage.
    pub provider: String,
    /// Model input tokens when reported.
    pub input_tokens: Option<u64>,
    /// Model output tokens when reported.
    pub output_tokens: Option<u64>,
    /// Provider credit units when reported.
    pub credits: Option<f64>,
    /// Normalized invocation cost when reported.
    pub cost_usd: Option<f64>,
    /// How normalized cost was obtained.
    pub cost_basis: Option<String>,
}

impl Usage {
    /// A known zero-cost execution.
    #[must_use]
    pub fn zero(provider: &str) -> Self {
        Self {
            provider: provider.to_owned(),
            input_tokens: Some(0),
            output_tokens: Some(0),
            credits: Some(0.0),
            cost_usd: Some(0.0),
            cost_basis: Some("known_zero".to_owned()),
        }
    }

    /// Usage with no measurable provider values.
    #[must_use]
    pub fn unknown(provider: &str) -> Self {
        Self {
            provider: provider.to_owned(),
            ..Self::default()
        }
    }

    /// Reads Claude Code's structured headless result envelope.
    #[must_use]
    pub fn from_claude_json(bytes: &[u8]) -> Self {
        let Some(value) = json(bytes) else {
            return Self::unknown("claude");
        };
        let usage = value.get("usage");
        Self {
            provider: "claude".to_owned(),
            input_tokens: integer(usage, "input_tokens"),
            output_tokens: integer(usage, "output_tokens"),
            credits: None,
            cost_usd: number(Some(&value), "total_cost_usd"),
            cost_basis: Some("provider_reported_total_cost_usd".to_owned()),
        }
    }

    /// Reads Copilot's file-exported OpenTelemetry JSON lines.
    #[must_use]
    pub fn from_copilot_otel(bytes: &[u8]) -> Self {
        let mut totals = Totals::default();
        for line in bytes.split(|byte| *byte == b'\n') {
            if let Some(value) = json(line) {
                scan(&value, &mut totals);
            }
        }
        totals.usage()
    }
}

#[derive(Default)]
struct Totals {
    input: u64,
    output: u64,
    nano_aiu: f64,
    saw_input: bool,
    saw_output: bool,
    saw_cost: bool,
}

impl Totals {
    fn usage(self) -> Usage {
        let credits = self.saw_cost.then_some(self.nano_aiu / 1_000_000_000.0);
        Usage {
            provider: "copilot".to_owned(),
            input_tokens: self.saw_input.then_some(self.input),
            output_tokens: self.saw_output.then_some(self.output),
            credits,
            cost_usd: credits.map(|value| value * 0.01),
            cost_basis: credits.map(|_| "github_ai_credit_at_usd_0.01".to_owned()),
        }
    }
}

fn scan(value: &serde_json::Value, totals: &mut Totals) {
    match value {
        serde_json::Value::Object(map) => {
            attribute_pair(map, totals);
            for (key, value) in map {
                add(key, value, totals);
                scan(value, totals);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                scan(value, totals);
            }
        }
        _ => {}
    }
}

fn attribute_pair(map: &serde_json::Map<String, serde_json::Value>, totals: &mut Totals) {
    let Some(key) = map.get("key").and_then(serde_json::Value::as_str) else {
        return;
    };
    if let Some(value) = map.get("value") {
        add(key, value, totals);
    }
}

fn add(key: &str, value: &serde_json::Value, totals: &mut Totals) {
    match key {
        "gen_ai.usage.input_tokens" => add_count(value, &mut totals.input, &mut totals.saw_input),
        "gen_ai.usage.output_tokens" => {
            add_count(value, &mut totals.output, &mut totals.saw_output);
        }
        "gen_ai.usage.total_nano_aiu" | "github.copilot.total_nano_aiu" | "total_nano_aiu" => {
            add_cost(value, totals);
        }
        _ => {}
    }
}

fn add_cost(value: &serde_json::Value, totals: &mut Totals) {
    if let Some(value) = numeric(value) {
        totals.nano_aiu += value;
        totals.saw_cost = true;
    }
}

/// The agent response nested in Claude's structured envelope.
#[must_use]
pub fn claude_result(bytes: &[u8]) -> Option<Vec<u8>> {
    json(bytes)?
        .get("result")?
        .as_str()
        .map(|result| result.as_bytes().to_vec())
}

/// A validated step result plus adapter-owned usage.
#[derive(Debug, Clone, PartialEq)]
pub struct Execution {
    /// Agent/deterministic result.
    pub result: StepResult,
    /// Usage measured by the adapter.
    pub usage: Usage,
    halt: bool,
}

impl Execution {
    /// Pairs a result with measured usage.
    #[must_use]
    pub const fn new(result: StepResult, usage: Usage) -> Self {
        Self {
            result,
            usage,
            halt: false,
        }
    }

    /// Marks a control failure that must not checkpoint or route.
    #[must_use]
    pub const fn halt(mut self) -> Self {
        self.halt = true;
        self
    }

    /// Whether execution must stop before checkpointing.
    #[must_use]
    pub const fn is_halted(&self) -> bool {
        self.halt
    }
}
