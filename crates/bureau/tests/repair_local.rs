//! Offline production-effect tests for guarded local repairs.

pub mod local_effects_support;

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::time::Duration;

use bureau::home::Directory;
use bureau::repair::{
    self, CacheState, Candidate, Confirmation, DerivedState, DirectoryState, DisposableCache,
    LocalEffects, Ownership, OwnershipState, PluginActivationState, WorktreeState,
};
use rusqlite::Connection;

use local_effects_support::Fixture;

#[test]
fn directories_and_explicit_unused_caches_are_repaired() {
    let fixture = Fixture::new("repair-files");
    let layout = fixture.layout();
    fs::remove_dir(layout.credentials()).expect("remove credentials");
    fs::set_permissions(layout.runs(), fs::Permissions::from_mode(0o755)).expect("loosen mode");
    fs::write(layout.checkout_cache().join("mirror"), "cache").expect("checkout cache");
    fs::write(layout.config_cache().join("snapshot"), "cache").expect("config cache");
    let _state = fixture.state();
    let plan = repair::plan(file_candidates());
    let summary = repair::run(plan, Confirmation::Approve, &mut LocalEffects::new(layout))
        .expect("repair files");
    let actual = (
        summary.applied,
        mode(layout.credentials()),
        mode(layout.runs()),
        empty(layout.checkout_cache()),
        empty(layout.config_cache()),
    );
    assert_eq!(actual, (4, 0o700, 0o700, true, true));
}

#[test]
fn live_run_evidence_blocks_cache_clearing() {
    let fixture = Fixture::new("repair-live");
    let cache_file = fixture.layout().checkout_cache().join("keep");
    fs::write(&cache_file, "live").expect("cache file");
    let state = fixture.state();
    state
        .try_claim_run(
            "assignment",
            "github",
            "42",
            "running",
            Duration::from_secs(60),
        )
        .expect("active claim");
    let _run = fixture.run("running", false, false);
    let plan = repair::plan([Candidate::Cache(CacheState {
        cache: DisposableCache::Checkout,
        in_use: false,
    })]);
    let result = repair::run(
        plan,
        Confirmation::Approve,
        &mut LocalEffects::new(fixture.layout()),
    );
    assert_eq!((result.is_err(), cache_file.exists()), (true, true));
}

#[test]
fn exact_expired_ownership_is_reaped() {
    let fixture = Fixture::new("repair-lease");
    let store = fixture.state();
    store
        .try_claim_run("assignment", "github", "42", "run-expired", Duration::ZERO)
        .expect("claim");
    let ownership = read_ownership(fixture.layout().state_db());
    let plan = repair::plan([Candidate::Ownership(OwnershipState {
        observed_at_ms: u64::MAX,
        ownership,
    })]);
    repair::run(
        plan,
        Confirmation::Approve,
        &mut LocalEffects::new(fixture.layout()),
    )
    .expect("reap");
    assert_eq!(lease_count(fixture.layout().state_db()), 0);
}

#[test]
fn orphan_worktree_and_finished_derived_state_are_repaired() {
    let fixture = Fixture::new("repair-recovery");
    let orphan = fixture.orphan_worktree("orphan");
    let finished = fixture.run("finished", true, false);
    let summary = repair::run(
        repair::plan(recovery_candidates()),
        Confirmation::Approve,
        &mut LocalEffects::new(fixture.layout()),
    )
    .expect("repair recovery");
    assert_eq!(
        (
            summary.applied,
            orphan.exists(),
            finished.join("state.json").is_file()
        ),
        (2, false, true)
    );
}

fn recovery_candidates() -> [Candidate; 2] {
    [
        Candidate::Worktree(WorktreeState {
            run_id: "orphan".to_owned(),
            run_exists: false,
            ownership_active: false,
        }),
        Candidate::DerivedState(DerivedState {
            run_id: "finished".to_owned(),
            durable_history_exists: true,
            needs_rebuild: true,
            run_active: false,
        }),
    ]
}

#[test]
fn interrupted_activation_restores_exact_original_state() {
    let fixture = Fixture::new("repair-activation");
    let run = fixture.run("durable", false, true);
    let worktree = run.join("wt");
    std::fs::create_dir_all(&worktree).expect("worktree");
    let activation = bureau::plugin::activate_direct("reviewer.md", b"agent", &worktree, &run)
        .expect("activate");
    let activation_id = restoration_id(&run);
    std::mem::forget(activation);
    let result = repair::run(
        repair::plan([activation_candidate(&activation_id)]),
        Confirmation::Approve,
        &mut LocalEffects::new(fixture.layout()),
    );
    assert_eq!(
        (
            result.is_ok(),
            run.join("events.jsonl").is_file(),
            run.join("activations").exists(),
            worktree.join(".github/agents/reviewer.agent.md").exists()
        ),
        (true, true, false, false)
    );
}

fn restoration_id(run: &std::path::Path) -> String {
    bureau::plugin::restoration_infos(run)
        .expect("restoration")
        .pop()
        .expect("activation")
        .activation_id
}

fn activation_candidate(activation_id: &str) -> Candidate {
    Candidate::PluginActivation(PluginActivationState {
        activation_id: activation_id.to_owned(),
        run_id: "durable".to_owned(),
        plugin: "reviewer".to_owned(),
        recorded_version: "pinned".to_owned(),
        installed_version: "pinned".to_owned(),
        stale: true,
        run_active: false,
    })
}

#[test]
fn durable_worktree_pruning_fails_closed() {
    let fixture = Fixture::new("repair-durable");
    let run = fixture.run("durable", true, true);
    let plan = repair::plan([Candidate::Worktree(WorktreeState {
        run_id: "durable".to_owned(),
        run_exists: false,
        ownership_active: false,
    })]);
    let result = repair::run(
        plan,
        Confirmation::Approve,
        &mut LocalEffects::new(fixture.layout()),
    );
    assert_eq!(
        (
            result.is_err(),
            run.join("events.jsonl").is_file(),
            run.join("wt").is_dir()
        ),
        (true, true, true)
    );
}

#[test]
fn registered_orphan_removes_files_and_git_registration() {
    let fixture = Fixture::new("repair-registered");
    let (mirror, worktree) = fixture.registered_worktree("registered");
    let plan = repair::plan([Candidate::Worktree(WorktreeState {
        run_id: "registered".to_owned(),
        run_exists: false,
        ownership_active: false,
    })]);
    repair::run(
        plan,
        Confirmation::Approve,
        &mut LocalEffects::new(fixture.layout()),
    )
    .expect("prune registered worktree");
    let registrations = mirror.join("worktrees");
    assert_eq!((worktree.exists(), registrations.exists()), (false, false));
}

fn file_candidates() -> Vec<Candidate> {
    vec![
        Candidate::Directory(DirectoryState {
            directory: Directory::Credentials,
            exists: false,
            permissions_ok: false,
        }),
        Candidate::Directory(DirectoryState {
            directory: Directory::Runs,
            exists: true,
            permissions_ok: false,
        }),
        Candidate::Cache(CacheState {
            cache: DisposableCache::Checkout,
            in_use: false,
        }),
        Candidate::Cache(CacheState {
            cache: DisposableCache::Config,
            in_use: false,
        }),
    ]
}

fn mode(path: &std::path::Path) -> u32 {
    fs::metadata(path).expect("metadata").permissions().mode() & 0o777
}

fn empty(path: &std::path::Path) -> bool {
    fs::read_dir(path).expect("read directory").next().is_none()
}

fn read_ownership(path: &std::path::Path) -> Ownership {
    Connection::open(path)
        .expect("open state")
        .query_row(
            "SELECT assignment, forge, external_id, run_id, owner_id, expires_at_ms FROM leases",
            (),
            |row| {
                Ok(Ownership {
                    assignment: row.get(0)?,
                    forge: row.get(1)?,
                    external_id: row.get(2)?,
                    run_id: row.get(3)?,
                    owner_id: row.get(4)?,
                    expires_at_ms: row.get(5)?,
                })
            },
        )
        .expect("ownership")
}

fn lease_count(path: &std::path::Path) -> usize {
    Connection::open(path)
        .expect("open state")
        .query_row("SELECT COUNT(*) FROM leases", (), |row| row.get(0))
        .expect("lease count")
}
