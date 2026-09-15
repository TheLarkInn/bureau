mod classification;

use std::collections::BTreeMap;
use std::path::PathBuf;

use bureau::config::{Assignment, Config, Pipeline};
use bureau::setup::Settings;
use serde_json::json;

use super::{for_assignment, for_assignments, for_pipeline};

fn pipeline(reference: &str) -> Pipeline {
    serde_json::from_value(json!({
        "name": "work", "steps": [{"name": "factory", "type": "agent", "role": "worker",
            "copilot_factory": {
                "name": "work", "extension": "project:work",
                "extension_digest": format!("tree-sha256:{}", "a".repeat(64)),
                "model_credential": reference,
                "runtime": {"profile": "copilot-sdk-factory-v1",
                    "directory": "/unused/qualified", "digest": format!("tree-sha256:{}", "b".repeat(64)),
                    "version": "unused", "executable": "node", "dist": "."}
            }, "next": "done"}]
    })).expect("model-only pipeline")
}

fn ordinary() -> Pipeline {
    serde_json::from_value(json!({
        "name": "ordinary", "steps": [{"name": "check", "type": "deterministic", "run": "true", "next": "done"}]
    })).expect("ordinary pipeline")
}

fn named_pipeline(name: &str, reference: &str) -> Pipeline {
    let mut value = pipeline(reference);
    value.name = name.to_owned();
    value
}

fn assignment(name: &str) -> Assignment {
    serde_json::from_value(json!({
        "name": name, "pipeline": name, "role": "worker", "repos": ["main"],
        "verify": "true", "branch_prefix": "bureau/",
        "work": {"forge": "github", "source": "fixture", "filter": "*"}
    }))
    .expect("assignment")
}

fn assigned(pipelines: Vec<Pipeline>) -> Config {
    Config {
        repos: BTreeMap::new(),
        roles: BTreeMap::new(),
        assignments: pipelines
            .iter()
            .map(|value| (value.name.clone(), assignment(&value.name)))
            .collect(),
        label_rules: BTreeMap::new(),
        pipelines: pipelines
            .into_iter()
            .map(|value| (value.name.clone(), value))
            .collect(),
    }
}

fn expected_errors() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "absent".into(),
            "credential `absent` is not declared in settings.yaml".into(),
        ),
        (
            "missing".into(),
            "credential `missing` is unavailable from its declared source".into(),
        ),
    ])
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let id = bureau::engine::new_run_id("model-source").expect("fixture identity");
        let path = std::env::temp_dir().join(format!("bureau-model-source-{id}"));
        std::fs::write(&path, "synthetic-model-secret").expect("dummy credential source");
        Self(path)
    }

    fn settings(&self) -> Settings {
        serde_json::from_value(json!({
            "config": {"kind": "single_repository", "remote": "fixture", "reference": "main"},
            "credentials": {
                "model-ref": {"source": "file", "path": self.0},
                "unrelated": {"source": "file", "path": self.0}
            }
        }))
        .expect("declared local sources")
    }

    fn missing_settings(&self) -> Settings {
        let mut settings = self.settings();
        settings.credentials.insert(
            "missing".into(),
            bureau::setup::CredentialSource::File {
                path: self.0.with_extension("missing"),
            },
        );
        settings
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).expect("remove dummy credential file");
    }
}

#[test]
fn a_model_only_pipeline_resolves_only_its_declared_reference() {
    let fixture = Fixture::new();
    let resolved =
        for_pipeline(&pipeline("model-ref"), Some(&fixture.settings())).expect("resolve");
    let names: Vec<_> = resolved.keys().map(String::as_str).collect();
    assert_eq!(
        (
            names,
            resolved["model-ref"].expose() == "synthetic-model-secret",
            format!("{resolved:?}").contains("synthetic-model-secret")
        ),
        (vec!["model-ref"], true, false),
    );
}

#[test]
fn an_environment_name_is_not_a_declared_model_credential_or_login_fallback() {
    let fixture = Fixture::new();
    for settings in [None, Some(fixture.settings())] {
        let error = for_pipeline(&pipeline("PATH"), settings.as_ref()).expect_err("no inference");
        assert!(error.to_string().contains("settings.yaml"), "{error}");
    }
}

#[test]
fn missing_declared_source_fails_without_substituting_another_credential() {
    let fixture = Fixture::new();
    let mut settings = fixture.settings();
    settings.credentials.insert(
        "model-ref".into(),
        bureau::setup::CredentialSource::File {
            path: fixture.0.with_extension("missing"),
        },
    );
    let error = for_pipeline(&pipeline("model-ref"), Some(&settings)).expect_err("missing source");
    assert!(error.to_string().contains("model-ref"), "{error}");
}

#[test]
fn ordinary_and_unassigned_pipelines_need_no_model_authentication() {
    let config = Config {
        repos: BTreeMap::new(),
        roles: BTreeMap::new(),
        assignments: BTreeMap::new(),
        label_rules: BTreeMap::new(),
        pipelines: BTreeMap::from([("work".into(), pipeline("absent"))]),
    };
    assert_eq!(
        (
            for_pipeline(&ordinary(), None).expect("ordinary").len(),
            for_assignments(&config, None)
                .expect("unassigned")
                .values
                .len()
        ),
        (0, 0),
    );
}

#[test]
fn batch_retains_each_failed_reference_and_independent_resolved_values() {
    let fixture = Fixture::new();
    let config = assigned(vec![
        pipeline("model-ref"),
        ordinary(),
        named_pipeline("unavailable", "missing"),
        named_pipeline("undeclared", "absent"),
        named_pipeline("shared", "missing"),
    ]);
    let resolved = for_assignments(&config, Some(&fixture.missing_settings())).expect("batch");
    assert_eq!(
        (
            resolved
                .values
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            resolved.errors
        ),
        (vec!["model-ref"], expected_errors()),
    );
}

#[test]
fn batch_without_settings_retains_errors_instead_of_inferring_environment_auth() {
    let config = assigned(vec![
        pipeline("PATH"),
        named_pipeline("other", "absent"),
        ordinary(),
    ]);
    let resolved = for_assignments(&config, None).expect("ordinary work remains available");
    assert_eq!(
        (
            resolved.values.len(),
            resolved
                .errors
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            resolved
                .errors
                .values()
                .all(|error| error.contains("settings.yaml")),
        ),
        (0, vec!["PATH", "absent"], true),
    );
}

#[test]
fn explicit_assignment_resolution_stays_strict_and_scoped() {
    let fixture = Fixture::new();
    let config = assigned(vec![pipeline("missing"), ordinary()]);
    let settings = fixture.missing_settings();
    let error = for_assignment(&config, &config.assignments["work"], Some(&settings))
        .expect_err("explicit factory cannot start");
    assert_eq!(
        (
            error.to_string(),
            for_assignment(&config, &config.assignments["ordinary"], Some(&settings))
                .expect("ordinary explicit assignment")
                .len(),
        ),
        (
            "credential `missing` is unavailable from its declared source".into(),
            0
        ),
    );
}

#[test]
fn batch_still_rejects_a_dangling_pipeline() {
    let mut config = assigned(vec![ordinary()]);
    config.pipelines.clear();
    let error = for_assignments(&config, None).expect_err("invalid assignment is not ignored");
    assert!(
        error
            .to_string()
            .contains("assignment `ordinary` has no pipeline `ordinary`")
    );
}

#[test]
fn resolving_a_later_revision_does_not_keep_an_old_source_failure() {
    let fixture = Fixture::new();
    let config = assigned(vec![pipeline("model-ref")]);
    let missing = for_assignments(&config, None).expect("retained missing settings");
    let restored = for_assignments(&config, Some(&fixture.settings())).expect("restored source");
    assert_eq!(
        (
            missing.errors.len(),
            restored.errors.len(),
            restored.values.len()
        ),
        (1, 0, 1),
    );
}
