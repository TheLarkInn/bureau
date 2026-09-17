use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bureau::config::{AdapterKind, Config, Pipeline, StepKind};
use bureau::contract::{StepOutcome, Trust};
use bureau::engine::{Engine, RunOutcome, RunPlan};
use bureau::forge::{Item, fake::FakeForge};
use bureau::runlog::{self, EventKind};

const NO_WORK: &str = r#"{
  "schema":"v2",
  "chunks":[{"delay_ms":0,"stream":"stdout","data":"{\"schema\":\"v2\",\"outcome\":\"no-work\",\"outputs\":{},\"artifacts\":[],\"trust\":\"derived\",\"message\":\"Offline no-work fixture.\"}\n"}],
  "exit_code":0,
  "usage":{"provider":"fake","input_tokens":0,"output_tokens":0,"credits":0,"cost_usd":0,"cost_basis":"known_zero"}
}"#;

fn git(directory: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(directory)
        .output()
        .expect("local git");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository(root: &Path) -> PathBuf {
    let repository = root.join("repository");
    std::fs::create_dir(&repository).expect("repository");
    git(&repository, &["init", "--quiet", "-b", "main"]);
    std::fs::write(repository.join("file.txt"), "original\n").expect("seed");
    git(&repository, &["add", "file.txt"]);
    git(
        &repository,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "seed",
        ],
    );
    repository
}

fn fake_steps(pipeline: &mut Pipeline, fixture: &Path) {
    for step in &mut pipeline.steps {
        if step.kind == StepKind::Agent {
            step.fixture = Some(fixture.to_string_lossy().into_owned());
        }
    }
}

fn fake_adapters(config: &mut Config, fixture: &Path) {
    for role in config.roles.values_mut() {
        role.adapter = AdapterKind::Fake;
        role.agent = "agents/offline.md".to_owned();
    }
    for pipeline in config.pipelines.values_mut() {
        fake_steps(pipeline, fixture);
    }
}

fn configuration(root: &Path, repository: &Path) -> Config {
    let mut request: super::model::Request =
        serde_yaml_ng::from_str(super::REQUEST_TEMPLATE).expect("template");
    request.assignment.verify = "printf verified >&2; exit 1".to_owned();
    request.repositories.get_mut("code").expect("repo").url =
        repository.to_string_lossy().into_owned();
    let draft = super::draft::fixed(&request).expect("generated scaffold");
    let directory = root.join("config");
    super::files::materialize(&directory, &draft).expect("config");
    let mut config = Config::load(&directory).expect("real loader");
    let fixture = root.join("no-work.json");
    std::fs::write(&fixture, NO_WORK).expect("fixture");
    fake_adapters(&mut config, &fixture);
    config
}

fn item() -> Item {
    Item {
        external_id: "42".to_owned(),
        title: "Verify the scaffold no-work route".to_owned(),
        body: "The deterministic check must still run.".to_owned(),
        url: "fake://item/42".to_owned(),
        labels: vec!["bureau:approved".to_owned()],
        trust: Trust::Maintainer,
    }
}

fn plan(mut config: Config) -> RunPlan {
    let assignment = config.assignments.into_values().next().expect("assignment");
    let pipeline = config
        .pipelines
        .remove(&assignment.pipeline)
        .expect("pipeline");
    RunPlan {
        run_id: bureau::engine::new_run_id("init-scaffold").expect("run ID"),
        assignment,
        pipeline,
        roles: config.roles,
        repos: config.repos,
        item: item(),
        forge: Arc::new(FakeForge::new(vec![item()])),
        credentials: BTreeMap::new(),
        config_source: None,
        plugin_sources: BTreeMap::new(),
        direct_agents: BTreeMap::new(),
        lease: None,
    }
}

fn check_verification(root: &Path, outcome: &RunOutcome) {
    let events = runlog::read_events(&root.join("runs").join(&outcome.run_id)).expect("events");
    let started: Vec<_> = events
        .iter()
        .filter(|event| event.kind == EventKind::StepStarted)
        .map(|event| event.data["step"].as_str().expect("step"))
        .collect();
    let output: String = events
        .iter()
        .filter_map(|event| event.data["data"].as_str())
        .collect();
    let verified_without_pr = output.contains("verified") && outcome.pr.is_none();
    assert_eq!(
        (outcome.outcome, verified_without_pr, started),
        (StepOutcome::Blocked, true, vec!["implement", "verify"]),
        "{}",
        outcome.message
    );
}

#[tokio::test]
async fn generated_writer_no_work_executes_the_existing_verify_command() {
    let temporary = super::files::Temporary::new(&std::env::temp_dir(), "init-scaffold-test")
        .expect("temporary");
    let repository = repository(temporary.path());
    let plan = plan(configuration(temporary.path(), &repository));
    let engine = Engine::new(
        temporary.path().join("runs"),
        temporary.path().join("cache"),
    );
    let outcome = engine.run(&plan).await;
    check_verification(temporary.path(), &outcome);
}
