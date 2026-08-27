//! A step may not take a terminal's name (DESIGN.md sections 7, 11).
//!
//! `engine::edge::resolve` matches `done`, `abort` and `escalate` before it
//! looks at `pipeline.steps`, so a bare terminal name in an edge is the
//! *terminal* wherever it is written and a step of that name is unreachable
//! through it. Validation used to read the two the other way round:
//! `check_edge` returns as soon as the target names a step, so a pipeline
//! carrying a step called `abort` passed every rule — reachability included,
//! which counted the edge as arriving — while the engine stopped the run
//! instead. Neither the load nor the run said the two disagreed.
//!
//! The canvas refuses the name on the way in (`web/step-refs.mjs`
//! `stepNameProblem`). A config already on disk needs the same rule, because
//! the canvas is not the only way to write one.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::config::Config;

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bureau-test-{}-{}-{tag}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const REPOS: &str = "repos:\n  odsp-web:\n    url: https://dev.azure.com/microsoft/Odsp/_git/odsp-web\n    forge: ado\n    access: push\n    credential: ado-main\n";
const ROLE: &str = "name: implementer\nagent: /bureau:implementer\nadapter: copilot\npermissions: [repo:read, repo:write, repo:push, pr:write]\nmin_trust: maintainer\n";

const ASSIGNMENT: &str = r#"
name: fix-flaky-tests
work:
  forge: ado
  source: "Odsp/odsp-web"
  filter: "[System.Tags] CONTAINS 'agent-eligible'"
  approval_label: agent-approved
  abort_label: bureau:failed
  escalate_label: bureau:needs-human
repos: [odsp-web]
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

/// A two-step pipeline whose second step carries `name`, reached from the
/// first. Every other rule is satisfied, so the only thing under test is the
/// name: the control below loads the same shape clean.
fn pipeline_naming(name: &str) -> String {
    format!(
        "name: fix-failing-test\nsteps:\n  - name: claim\n    type: deterministic\n    run: bureau claim --next\n    next: {name}\n    on_no_work: done\n  - name: {name}\n    type: deterministic\n    run: scripts/publish.sh\n    next: done\n"
    )
}

fn load(tag: &str, step: &str) -> Result<Config, Vec<String>> {
    let dir = TestDir::new(tag);
    let files = [
        ("repos.yaml", REPOS),
        ("roles/implementer.yaml", ROLE),
        ("assignments/fix-flaky-tests.yaml", ASSIGNMENT),
        ("pipelines/fix-failing-test.yaml", &pipeline_naming(step)),
    ];
    for (name, text) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().expect("parent dir")).expect("mkdir");
        std::fs::write(path, text).expect("write fixture");
    }
    Config::load(dir.path()).map_err(|errors| errors.iter().map(ToString::to_string).collect())
}

/// Held as a rule over all three terminals rather than through whichever one
/// a caller happened to write, because the engine matches all three the same
/// way and a rule proved for one of them is a rule proved for none.
#[test]
fn a_step_named_after_a_terminal_is_refused() {
    let refused: Vec<bool> = ["done", "abort", "escalate"]
        .iter()
        .map(|terminal| {
            let found = load(terminal, terminal).expect_err("config must fail");
            found.iter().any(|error| {
                error.contains(&format!("step `{terminal}`")) && error.contains("is a terminal")
            })
        })
        .collect();

    assert_eq!(refused, vec![true, true, true]);
}

/// The control, and not a courtesy: without it the refusal above would be
/// satisfied by a fixture that fails to load for some unrelated reason, which
/// is the shape of pass this repository keeps finding. The same pipeline with
/// an ordinary name must load, and the step must survive into the config.
#[test]
fn the_same_pipeline_with_an_ordinary_name_still_loads() {
    let config = load("ordinary", "publish").expect("valid config");
    let steps: Vec<&str> = config.pipelines["fix-failing-test"]
        .steps
        .iter()
        .map(|step| step.name.as_str())
        .collect();

    assert_eq!(steps, vec!["claim", "publish"]);
}
