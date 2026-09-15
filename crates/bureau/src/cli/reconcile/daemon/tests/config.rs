use std::path::Path;

use bureau::setup::Settings;
use serde_json::{Value, json};

fn assignment(name: &str) -> Value {
    json!({
        "name": name, "pipeline": name, "role": "worker", "repos": ["main"],
        "verify": "true", "branch_prefix": "bureau/",
        "work": {"forge": "github", "source": "offline/work", "filter": "*",
            "abort_label": "failed", "escalate_label": "needs-human"},
        "limits": {"max_concurrent": 1, "max_runs_per_hour": 1}
    })
}

fn factory(root: &Path) -> Value {
    json!({
        "name": "factory", "steps": [{"name": "work", "type": "agent", "role": "worker",
            "copilot_factory": {
                "name": "work", "extension": "project:work", "model_credential": "missing-model",
                "extension_digest": format!("tree-sha256:{}", "a".repeat(64)),
                "runtime": {"profile": "copilot-sdk-factory-v1",
                    "directory": root.join("never-activated"),
                    "digest": format!("tree-sha256:{}", "b".repeat(64)),
                    "version": "unused", "executable": "node", "dist": "."}
            }, "next": "done"}]
    })
}

fn labels() -> Value {
    json!({
        "name": "graduate", "work": {
            "forge": "github", "source": "offline/work", "filter": "label:blocked"
        },
        "when": "dependencies_closed", "add_labels": ["eligible"], "remove_labels": ["blocked"],
        "limits": {"max_updates_per_hour": 5}
    })
}

fn registry(remote: &Path) -> Value {
    json!({"repos": {
        "main": {"url": remote, "forge": "github", "access": "push", "credential": "git-main"},
        "labels": {"url": "https://github.com/offline/work", "forge": "github",
            "access": "read", "credential": "git-main"}
    }})
}

fn role() -> Value {
    json!({
        "name": "worker", "agent": "/fixture:worker", "adapter": "copilot",
        "permissions": ["repo:read", "model:invoke"], "min_trust": "untrusted"
    })
}

fn ordinary() -> Value {
    json!({
        "name": "ordinary", "steps": [
            {"name": "check", "type": "deterministic", "run": "true", "next": "done"}
        ]
    })
}

fn write(root: &Path, relative: &str, value: &Value) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture directories");
    std::fs::write(path, serde_yaml_ng::to_string(value).expect("fixture yaml"))
        .expect("fixture file");
}

pub(super) fn write_config(root: &Path, remote: &Path) {
    let config = root.join(".bureau");
    write(&config, "repos.yaml", &registry(remote));
    write(&config, "roles/worker.yaml", &role());
    for name in ["factory", "ordinary"] {
        write(
            &config,
            &format!("assignments/{name}.yaml"),
            &assignment(name),
        );
    }
    write(&config, "pipelines/factory.yaml", &factory(root));
    write(&config, "pipelines/ordinary.yaml", &ordinary());
    write(&config, "label_rules/graduate.yaml", &labels());
}

pub(super) fn settings(root: &Path) -> Settings {
    serde_json::from_value(json!({
        "config": {"kind": "single_repository", "remote": "fixture", "reference": "main"},
        "credentials": {
            "git-main": {"source": "file", "path": root.join("repo-credential")},
            "missing-model": {"source": "file", "path": root.join("missing-model-credential")}
        }
    }))
    .expect("fixture settings")
}
