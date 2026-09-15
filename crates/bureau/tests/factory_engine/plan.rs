use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use bureau::config::{CopilotFactory, Pipeline, Role};
use bureau::engine::{RunPlan, new_run_id};
use bureau::forge::fake::FakeForge;
use bureau::process::Secret;
use serde_json::{Value, json};

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).expect("fixture configuration")
}

fn factory(repository: &Path, runtime: &Path, mode: &str) -> CopilotFactory {
    let provider = repository.join(".github/extensions/offline-provider");
    let dist = if mode == "root-dist" { "." } else { "dist" };
    decode(json!({
        "name": "offline-factory", "extension": "project:offline-provider",
        "model_credential": "copilot-model",
        "extension_digest": bureau_plugin::tree_digest(&provider).expect("provider digest"),
        "runtime": {
            "profile": "copilot-sdk-factory-v1",
            "directory": runtime,
            "digest": bureau_plugin::tree_digest(runtime).expect("runtime digest"),
            "version": "offline-fixture-c", "executable": "fake-runtime", "dist": dist
        },
        "args": {"mode": mode}, "limits": {"max_total_subagents": 3, "max_ai_credits": 4.0}
    }))
}

fn pipeline(repository: &Path, runtime: &Path, mode: &str) -> Pipeline {
    let factory = factory(repository, runtime, mode);
    decode(json!({"name":"offline", "steps":[{
        "name": "factory-step", "type": "agent", "role": "worker",
        "copilot_factory": factory, "next": "done", "on_failure": "abort",
        "timeout_secs": 10, "max_attempts": 1
    }]}))
}

fn role() -> Role {
    decode(
        json!({"name":"worker", "agent":"agents/worker.md", "adapter":"copilot",
        "permissions":["model:invoke"], "min_trust":"untrusted"}),
    )
}

fn direct_agents() -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([(
        "worker".into(),
        b"---\nname: worker\ndescription: Offline worker\n---\nUse approved tools only.\n".to_vec(),
    )])
}

fn assignment() -> bureau::config::Assignment {
    decode(json!({
        "name":"offline", "work":{"forge":"github", "source":"fixture/repo", "filter":"*",
            "abort_label":"bureau:failed", "escalate_label":"bureau:needs-human"},
        "repos":["main"], "pipeline":"offline", "role":"worker", "verify":"true",
        "branch_prefix":"bureau/", "limits":{"max_cost_per_day_usd":10.0}
    }))
}

pub fn build(repository: &Path, runtime: &Path, mode: &str) -> RunPlan {
    let item = decode(
        json!({"external_id":"1", "title":"Offline fixture", "body":"",
        "url":"https://example.invalid/1", "labels":[], "trust":"maintainer"}),
    );
    let repo = decode(
        json!({"url": repository, "forge":"github", "access":"read", "credential":"unused"}),
    );
    RunPlan {
        run_id: new_run_id("offline").expect("run id"),
        assignment: assignment(),
        pipeline: pipeline(repository, runtime, mode),
        roles: BTreeMap::from([("worker".into(), role())]),
        repos: BTreeMap::from([("main".into(), repo)]),
        item,
        forge: Arc::new(FakeForge::new(Vec::new())),
        credentials: BTreeMap::from([("copilot-model".into(), Secret::new("offline-model-token"))]),
        config_source: None,
        plugin_sources: BTreeMap::new(),
        direct_agents: direct_agents(),
        lease: None,
    }
}
