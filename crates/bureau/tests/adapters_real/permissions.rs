use super::*;

fn assert_sandboxed(req: &SpawnRequest) {
    assert_eq!(&req.argv[5..7], ["--experimental", "--sandbox"]);
    assert!(!req.argv.iter().any(|arg| arg == "--allow-all-paths"));
}

fn permission_args(req: &SpawnRequest) -> String {
    let at = req
        .argv
        .iter()
        .position(|arg| arg == "--additional-mcp-config")
        .expect("mcp config present");
    req.argv[at + 2..].join(SEP)
}

fn assert_scoped(permissions: &[Permission], push: bool, gh: bool) {
    let dir = TestDir::new("copilot-flags");
    let role = role("/p:a", AdapterKind::Copilot, permissions);
    let req = copilot_request(&role, &step(None), dir.path());
    let mut expected = vec![
        "--allow-tool=write".to_owned(),
        "--allow-tool=shell".to_owned(),
        "--add-dir".to_owned(),
        dir.path().to_string_lossy().into_owned(),
    ];
    if !push {
        expected.push("--deny-tool=shell(git push)".to_owned());
    }
    if !gh {
        expected.push("--deny-tool=shell(gh:*)".to_owned());
    }
    assert_sandboxed(&req);
    assert_eq!(permission_args(&req), expected.join(SEP));
}

fn assert_denied(permissions: &[Permission]) {
    let dir = TestDir::new("copilot-denied");
    let role = role("/p:a", AdapterKind::Copilot, permissions);
    let req = copilot_request(&role, &step(None), dir.path());
    assert_sandboxed(&req);
    assert_eq!(permission_args(&req), "--deny-tool=shell(*)");
}

#[test]
fn repo_write_scopes_the_worktree_and_denies_push() {
    assert_scoped(&[Permission::RepoWrite], false, false);
}

#[test]
fn repo_push_scopes_the_worktree_and_allows_push() {
    assert_scoped(&[Permission::RepoPush], true, true);
}

#[test]
fn issues_write_allows_gh_without_allowing_push() {
    assert_scoped(
        &[Permission::RepoWrite, Permission::IssuesWrite],
        false,
        true,
    );
}

#[test]
fn roles_without_write_deny_shell() {
    assert_denied(&[Permission::RepoRead]);
    assert_denied(&[]);
}
