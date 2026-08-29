//! A step may not take a terminal's name.
//!
//! `engine::edge::resolve` matches `done`, `abort` and `escalate` before it
//! looks at `pipeline.steps`, so a bare terminal name in an edge is the
//! terminal wherever it is written and a step of that name is unreachable
//! through it. Validation used to read the two the other way round:
//! `check_edge` returns as soon as the target names a step, so a pipeline
//! carrying a step called `abort` passed every rule — reachability included,
//! which counted the edge as arriving — while the engine stopped the run
//! instead. Neither the load nor the run said the two disagreed.
//!
//! The canvas refuses the name on the way in (`web/step-refs.mjs`
//! `stepNameProblem`). A config already on disk needs the same rule, because
//! the canvas is not the only way to write one.
//!
//! Sharing the parent's binary and harness, as `engine/terminal_labels.rs` and
//! `label_rules/recovery.rs` do: this is a sub-rule of the parent's subject,
//! and a standalone file would copy `TestDir` and three fixtures verbatim to
//! buy a seventieth link unit.

use bureau::config::Config;

use super::{ASSIGNMENT, REPOS, ROLE_IMPLEMENTER, TestDir, write_files};

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
    write_files(
        &dir,
        &[
            ("repos.yaml", REPOS),
            ("roles/implementer.yaml", ROLE_IMPLEMENTER),
            ("assignments/fix-flaky-tests.yaml", ASSIGNMENT),
            ("pipelines/fix-failing-test.yaml", &pipeline_naming(step)),
        ],
    );
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
