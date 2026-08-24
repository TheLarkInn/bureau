//! What a step's *logged* agent identity may and may not do.

use super::*;

/// The run log names a step's agent before `plugins::activate` has captured the
/// worktree's originals, so that identity has to be computed rather than
/// materialized.
///
/// `expected_agent` is the pure form: the same name the adapter will pass to
/// `--agent`, with no discovery file written. Resolving it the side-effecting
/// way — `resolved_agent`, which the spawn path uses on purpose — would copy
/// the agent into the worktree first. The guard would then record that copy as
/// an *original* and restore it instead of deleting it, and the checkpoint's
/// `git add -A` would commit it onto the run branch.
#[test]
fn naming_an_md_path_agent_for_the_log_materializes_nothing() {
    let dir = TestDir::new("pure-identity");
    let agent = dir.path().join("notes.md");
    std::fs::write(&agent, AGENT_BODY).expect("write agent");
    let role = role(
        agent.to_str().expect("utf8 path"),
        AdapterKind::Copilot,
        &[],
    );

    let named = bureau::adapters::expected_agent(&role);

    assert_eq!(
        (named.as_str(), dir.path().join(".github/agents").exists()),
        ("notes", false)
    );
}

/// The pure name and the spawn path's resolved name have to agree, or the
/// canvas would draw a mismatch badge on a run that did exactly what its config
/// asked. They are only safe to swap because they are equal here.
#[test]
fn the_logged_name_is_the_one_the_adapter_invokes() {
    let dir = TestDir::new("identity-agrees");
    let agent = dir.path().join("notes.md");
    std::fs::write(&agent, AGENT_BODY).expect("write agent");
    let role = role(
        agent.to_str().expect("utf8 path"),
        AdapterKind::Copilot,
        &[],
    );
    let request = copilot_request(&role, &step(None), dir.path());

    assert_eq!(
        bureau::adapters::expected_agent(&role),
        value_after(&request.argv, "--agent")
    );
}

/// Why the loader's rule about `agent` is load-bearing rather than tidiness.
///
/// "The two agree for every config that validates" is a claim about the pair,
/// and the reason it holds is `config::validate`: an `agent` that is neither a
/// plugin invocation nor a `.md` path is a config error, pinned by
/// `an_agent_must_be_a_plugin_invocation_or_a_path`. This is that claim's other
/// half. For exactly the shape the loader refuses, the two forms genuinely
/// disagree — `agent_name` takes the file stem, `resolve_agent` passes the
/// reference through verbatim — so loosening the rule would put a name in the
/// run log that the adapter never invoked, and the canvas would report the
/// disagreement as config drift.
#[test]
fn the_two_forms_diverge_on_the_shape_the_loader_rejects() {
    let dir = TestDir::new("identity-diverges");
    let role = role("agents/notes", AdapterKind::Copilot, &[]);

    assert_eq!(
        (
            bureau::adapters::expected_agent(&role),
            bureau::adapters::resolved_agent(&role, dir.path()),
        ),
        ("notes".to_owned(), "agents/notes".to_owned())
    );
}
