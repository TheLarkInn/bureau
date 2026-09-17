//! Shipped setup trees use the production loader, without forge or model calls.

#[path = "scenarios/prerequisites.rs"]
mod prerequisites;

use std::collections::BTreeSet;
use std::path::PathBuf;

use bureau::config::{AdapterKind, Assignment, Config, Permission, Pipeline, StepKind};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    id: String,
    title: String,
    path: String,
    docs_anchor: String,
    execution: String,
    readiness: String,
    summary: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    schema: String,
    scenarios: Vec<Scenario>,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/scenarios")
}

fn catalog() -> Catalog {
    let text = std::fs::read_to_string(root().join("catalog.json")).expect("catalog");
    serde_json::from_str(&text).expect("catalog schema")
}

fn config(id: &str) -> Config {
    Config::load(&root().join(id)).expect("scenario must pass the real config loader")
}

fn expected_ids() -> BTreeSet<String> {
    [
        "design-review",
        "issue-intake",
        "issue-triage",
        "customer-feedback",
        "failing-test-repair",
        "multi-repo-fix",
        "azure-devops",
        "local-sdk-factory",
        "cloud-automation",
        "recurring-maintenance",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn directories() -> BTreeSet<String> {
    std::fs::read_dir(root())
        .expect("scenario directories")
        .map(|entry| entry.expect("directory entry"))
        .filter(|entry| entry.file_type().expect("file type").is_dir())
        .map(|entry| entry.file_name().into_string().expect("UTF-8 directory"))
        .collect()
}

fn expected_mode(id: &str) -> (&str, &str) {
    match id {
        "issue-intake" => ("label-reconciliation", "customization-required"),
        "local-sdk-factory" => ("qualified-local-factory", "qualification-required"),
        "cloud-automation" => ("existing-cloud-control", "eligibility-required"),
        _ => ("pipeline", "customization-required"),
    }
}

fn check_metadata(scenario: &Scenario, docs: &str) {
    assert_eq!(
        (&scenario.path, &scenario.docs_anchor),
        (&scenario.id, &scenario.id)
    );
    let mode = (scenario.execution.as_str(), scenario.readiness.as_str());
    assert_eq!(mode, expected_mode(&scenario.id));
    let heading = format!("## {}", scenario.title);
    assert_eq!(
        (
            docs.lines().any(|line| line == heading),
            scenario.summary.trim().is_empty()
        ),
        (true, false)
    );
}

fn check_assignment(assignment: &Assignment) {
    let limits = serde_json::to_value(&assignment.limits).expect("limits");
    let bounded = limits
        .as_object()
        .expect("limits object")
        .values()
        .all(serde_json::Value::is_number);
    let excluded = [
        &assignment.work.abort_label,
        &assignment.work.escalate_label,
    ]
    .into_iter()
    .all(|label| assignment.work.filter.contains(label));
    assert_eq!(
        (bounded, excluded, assignment.work.approval_label.is_some()),
        (true, true, true),
        "{} must bound spending and require separate approval",
        assignment.name
    );
}

fn verifier_target(pipeline: &Pipeline, target: Option<&str>) -> bool {
    pipeline
        .steps
        .iter()
        .find(|step| Some(step.name.as_str()) == target)
        .is_some_and(|step| matches!(step.kind, StepKind::Deterministic | StepKind::Concurrent))
}

fn check_write_gates(config: &Config, pipeline: &Pipeline) {
    for step in &pipeline.steps {
        let writes = step
            .role
            .as_ref()
            .and_then(|name| config.roles.get(name))
            .is_some_and(|role| role.permissions.contains(&Permission::RepoWrite));
        if writes {
            assert_eq!(
                (
                    verifier_target(pipeline, step.next.as_deref()),
                    verifier_target(pipeline, step.on_no_work.as_deref())
                ),
                (true, true),
                "{} cannot bypass verification with success or no-work",
                step.name
            );
        }
    }
}

fn has_verifier(assignment: &Assignment, pipeline: &Pipeline) -> bool {
    pipeline.steps.iter().any(|step| {
        step.kind == StepKind::Deterministic
            && step
                .run
                .as_deref()
                .is_some_and(|run| run.trim() == assignment.verify.trim())
    })
}

#[test]
fn catalog_names_exactly_ten_distinct_setup_trees() {
    let catalog = catalog();
    let ids: BTreeSet<_> = catalog
        .scenarios
        .iter()
        .map(|scenario| scenario.id.clone())
        .collect();
    assert_eq!(
        (
            catalog.schema.as_str(),
            catalog.scenarios.len(),
            ids,
            directories()
        ),
        ("bureau-scenarios-v1", 10, expected_ids(), expected_ids())
    );
}

#[test]
fn catalog_links_match_documented_headings_and_readiness() {
    let docs = std::fs::read_to_string(root().join("../../docs/scenarios.md")).expect("guide");
    for scenario in catalog().scenarios {
        check_metadata(&scenario, &docs);
    }
}

#[test]
fn every_tree_and_direct_agent_loads_without_network_or_models() {
    for scenario in catalog().scenarios {
        let config = config(&scenario.id);
        Config::load_agent_files(&root().join(&scenario.path), &config.roles)
            .expect("all direct agents must be included in their setup");
        assert!(
            config
                .roles
                .values()
                .all(|role| role.adapter != AdapterKind::Fake),
            "production setup must not silently substitute a fake"
        );
    }
}

#[test]
fn assignments_have_budgets_approval_and_terminal_filters() {
    for scenario in catalog().scenarios {
        for assignment in config(&scenario.id).assignments.values() {
            check_assignment(assignment);
        }
    }
}

#[test]
fn write_steps_cannot_publish_without_a_deterministic_check() {
    for scenario in catalog().scenarios {
        let config = config(&scenario.id);
        for pipeline in config.pipelines.values() {
            check_write_gates(&config, pipeline);
        }
    }
}

#[test]
fn ordinary_assignments_wire_their_actual_verification_command() {
    let scenarios = catalog()
        .scenarios
        .into_iter()
        .filter(|scenario| scenario.execution == "pipeline");
    for scenario in scenarios {
        let config = config(&scenario.id);
        for assignment in config.assignments.values() {
            assert!(
                has_verifier(assignment, &config.pipelines[&assignment.pipeline]),
                "{} must execute its declared verification",
                assignment.name
            );
        }
    }
}

#[test]
fn agents_receive_no_forge_mutation_grants() {
    let forbidden = [
        Permission::RepoPush,
        Permission::IssuesWrite,
        Permission::PrWrite,
        Permission::PrReview,
        Permission::PrMerge,
    ];
    for scenario in catalog().scenarios {
        let config = config(&scenario.id);
        let mut permissions = config.roles.values().flat_map(|role| &role.permissions);
        assert!(!permissions.any(|permission| forbidden.contains(permission)));
    }
}

#[test]
fn label_graduation_never_grants_approval_or_starts_a_pipeline() {
    let config = config("issue-intake");
    let rule = &config.label_rules["graduate-unblocked"];
    assert_eq!(
        (
            config.assignments.len(),
            config.pipelines.len(),
            rule.add_labels.as_slice()
        ),
        (0, 0, &["agent-eligible".to_owned()][..])
    );
}
