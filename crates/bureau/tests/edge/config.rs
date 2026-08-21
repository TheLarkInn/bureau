//! Config-loader adversarial edges (DESIGN.md sections 5-6): duplicate
//! YAML keys, anchors, wrong-shaped files, and permission typos.

use bureau::config::{Access, Config};

use super::testdir::TestDir;

const REPOS: &str = r"
repos:
  odsp-web:
    url: https://dev.azure.com/microsoft/Odsp/_git/odsp-web
    forge: ado
    access: push
    credential: ado-main
  augloop:
    url: https://dev.azure.com/office/Augmentation/_git/augloop
    forge: ado
    access: read
    credential: ado-main
";

const ROLE: &str = r"
name: implementer
agent: /bureau:implementer
adapter: copilot
permissions: [repo:read, repo:write, repo:push, pr:write]
min_trust: maintainer
";

const PIPELINE: &str = r#"
name: fix-failing-test
steps:
  - name: work
    type: deterministic
    run: "true"
    next: done
"#;

const ASSIGNMENT: &str = r#"
name: fix-flaky-tests
work:
  forge: ado
  source: "Odsp/odsp-web"
  filter: "[System.WorkItemType] = 'Bug'"
  abort_label: bureau:failed
  escalate_label: bureau:needs-human
repos: [odsp-web, augloop]
pipeline: fix-failing-test
role: implementer
verify: "rush test --to odsp-web"
branch_prefix: runner/
limits:
  max_concurrent: 2
  max_runs_per_hour: 6
  max_runs_per_day: 40
  max_open_prs: 5
  max_cost_per_day_usd: 25
"#;

fn write_files(dir: &TestDir, files: &[(&str, &str)]) {
    for (name, text) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().expect("parent dir")).expect("mkdir");
        std::fs::write(path, text).expect("write fixture");
    }
}

fn errors(dir: &TestDir) -> Vec<String> {
    Config::load(dir.path())
        .expect_err("config must fail")
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn a_duplicate_key_in_a_role_is_rejected() {
    let dir = TestDir::new("dupfield");
    let role = ROLE.replace("adapter: copilot", "adapter: copilot\nadapter: fake");
    write_files(
        &dir,
        &[("repos.yaml", REPOS), ("roles/implementer.yaml", &role)],
    );
    let found = errors(&dir);
    assert!(
        found
            .iter()
            .any(|e| e.contains("duplicate field") && e.contains("implementer.yaml")),
        "{found:?}"
    );
}

#[test]
fn a_duplicate_repo_key_is_silently_last_write_wins() {
    // KNOWN DELTA (README.md "Known deltas"): serde_yaml_ng 0.10 has no
    // duplicate-key rejection — its `Deserializer` takes no options and
    // `Mapping::insert` overwrites — so the LAST `odsp-web` entry (the
    // read-only one below) silently wins. repos.yaml carries the whole
    // authorization model; review config diffs carefully.
    let dir = TestDir::new("dupmap");
    let repos = REPOS.replace("  augloop:", "  odsp-web:");
    write_files(&dir, &[("repos.yaml", &repos)]);
    let config = Config::load(dir.path()).expect("last entry wins");
    assert_eq!(config.repos.len(), 1);
    assert_eq!(config.repos["odsp-web"].access, Access::Read);
}

#[test]
fn anchors_and_aliases_in_a_role_file_load() {
    let dir = TestDir::new("anchors");
    let repos = REPOS
        .replacen(
            "credential: ado-main",
            "credential: &credential ado-main",
            1,
        )
        .replacen("credential: ado-main", "credential: *credential", 1);
    let files: [(&str, &str); 4] = [
        ("repos.yaml", &repos),
        ("roles/implementer.yaml", ROLE),
        ("assignments/fix-flaky-tests.yaml", ASSIGNMENT),
        ("pipelines/fix-failing-test.yaml", PIPELINE),
    ];
    write_files(&dir, &files);
    // serde_yaml_ng resolves YAML anchors/aliases; nothing in DESIGN.md
    // restricts YAML features, and PR review sees the aliased source.
    let config = Config::load(dir.path()).expect("aliases resolve");
    assert_eq!(config.repos["augloop"].credential, "ado-main");
}

#[test]
fn a_role_file_that_is_a_list_is_a_parse_error() {
    let dir = TestDir::new("rolelist");
    let files: [(&str, &str); 2] = [
        ("repos.yaml", REPOS),
        ("roles/implementer.yaml", "- just\n- a\n- list\n"),
    ];
    write_files(&dir, &files);
    let found = errors(&dir);
    assert!(
        found
            .iter()
            .any(|e| e.contains("implementer.yaml") && e.contains("invalid type")),
        "{found:?}"
    );
}

#[test]
fn an_empty_roles_dir_makes_the_reference_an_error_not_a_panic() {
    let dir = TestDir::new("noroles");
    let files: [(&str, &str); 3] = [
        ("repos.yaml", REPOS),
        ("assignments/fix-flaky-tests.yaml", ASSIGNMENT),
        ("pipelines/fix-failing-test.yaml", PIPELINE),
    ];
    write_files(&dir, &files);
    std::fs::create_dir_all(dir.path().join("roles")).expect("mkdir roles");
    let found = errors(&dir);
    assert!(
        found
            .iter()
            .any(|e| e.contains("unknown role `implementer`")),
        "{found:?}"
    );
}

#[test]
fn a_permission_typo_names_the_file_and_token() {
    let dir = TestDir::new("permtypo");
    let role = ROLE.replace("pr:write]", "pr:writee]");
    write_files(
        &dir,
        &[("repos.yaml", REPOS), ("roles/implementer.yaml", &role)],
    );
    let found = errors(&dir);
    assert!(
        found
            .iter()
            .any(|e| e.contains("implementer.yaml") && e.contains("pr:writee")),
        "{found:?}"
    );
}
