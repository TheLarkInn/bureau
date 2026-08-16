//! Offline transcript tests for conservative repair planning and execution.

use bureau_lifecycle::home::Directory;
use bureau_lifecycle::repair::{
    self, Action, CacheState, Candidate, Confirmation, DerivedState, DirectoryState,
    DisposableCache, Ownership, OwnershipState, PluginActivationState, SkipReason, State,
    WorktreeState,
};

#[derive(Default)]
struct RepairEffects {
    applied: Vec<Action>,
    fail_on: Option<Action>,
}

impl RepairEffects {
    fn record(&mut self, action: Action) -> Result<(), String> {
        if self.fail_on.as_ref() == Some(&action) {
            return Err("injected failure".to_owned());
        }
        self.applied.push(action);
        Ok(())
    }
}

impl repair::Effects for RepairEffects {
    fn create_directory(&mut self, directory: Directory) -> Result<(), String> {
        self.record(Action::CreateDirectory { directory })
    }

    fn fix_directory_permissions(&mut self, directory: Directory) -> Result<(), String> {
        self.record(Action::FixDirectoryPermissions { directory })
    }

    fn clear_cache(&mut self, cache: DisposableCache) -> Result<(), String> {
        self.record(Action::ClearCache { cache })
    }

    fn restore_plugin_activation(
        &mut self,
        run_id: &str,
        activation_id: &str,
        plugin: &str,
        version: &str,
    ) -> Result<(), String> {
        self.record(Action::RestorePluginActivation {
            run_id: run_id.to_owned(),
            activation_id: activation_id.to_owned(),
            plugin: plugin.to_owned(),
            version: version.to_owned(),
        })
    }

    fn reap_expired_ownership(&mut self, ownership: &Ownership) -> Result<(), String> {
        self.record(Action::ReapExpiredOwnership {
            ownership: ownership.clone(),
        })
    }

    fn prune_orphan_worktree(&mut self, run_id: &str) -> Result<(), String> {
        self.record(Action::PruneOrphanWorktree {
            run_id: run_id.to_owned(),
        })
    }

    fn rebuild_derived_state(&mut self, run_id: &str) -> Result<(), String> {
        self.record(Action::RebuildDerivedState {
            run_id: run_id.to_owned(),
        })
    }
}

#[test]
fn approved_plan_applies_every_reversible_effect_in_order() {
    let plan = repair::plan(safe_candidates());
    let expected = plan.actions().to_vec();
    let mut effects = RepairEffects::default();
    let summary = repair::run(plan, Confirmation::Approve, &mut effects).expect("repair");
    assert_eq!(
        (summary.state, summary.applied, effects.applied),
        (State::Complete, 7, expected)
    );
}

#[test]
fn execution_is_impossible_before_confirmation_and_decline_is_empty() {
    let plan = repair::plan(directory_candidates());
    let mut machine = repair::Machine::new(plan);
    let mut effects = RepairEffects::default();
    let before = machine.apply_next(&mut effects);
    machine.confirm(Confirmation::Decline).expect("decline");
    let summary = machine.finish().expect("summary");
    assert_eq!(
        (before, summary, effects.applied,),
        (
            Err(repair::Error::ConfirmationRequired),
            repair::Summary {
                state: State::Declined,
                applied: 0,
            },
            Vec::new(),
        )
    );
}

#[test]
fn planner_excludes_live_version_changing_and_history_losing_work() {
    let plan = repair::plan(unsafe_candidates());
    let reasons: Vec<_> = plan.skipped().iter().map(|item| item.reason).collect();
    assert_eq!(
        (plan.actions().is_empty(), reasons),
        (
            true,
            vec![
                SkipReason::LiveWork,
                SkipReason::OwnershipNotExpired,
                SkipReason::LiveWork,
                SkipReason::PluginVersionChanged,
                SkipReason::LiveWork,
                SkipReason::DurableHistoryUnavailable,
                SkipReason::LiveWork,
            ],
        )
    );
}

#[test]
fn effect_failure_stops_without_applying_later_actions() {
    let plan = repair::plan(directory_candidates());
    let failure = plan.actions()[0].clone();
    let mut effects = RepairEffects {
        applied: Vec::new(),
        fail_on: Some(failure),
    };
    let result = repair::run(plan, Confirmation::Approve, &mut effects);
    assert_eq!(
        (
            matches!(result, Err(repair::Error::Effect { .. })),
            effects.applied
        ),
        (true, Vec::new())
    );
}

#[test]
fn plan_order_is_input_independent_and_duplicate_free() {
    let candidates = directory_candidates();
    let expected = repair::plan(candidates.clone());
    let mut reordered = candidates.clone();
    reordered.reverse();
    reordered.extend(candidates);
    let actual = repair::plan(reordered);
    assert_eq!((actual.actions().len(), actual), (2, expected));
}

fn safe_candidates() -> Vec<Candidate> {
    directory_candidates()
        .into_iter()
        .chain(cache_and_plugin_candidates())
        .chain(recovery_candidates())
        .collect()
}

fn directory_candidates() -> Vec<Candidate> {
    vec![
        Candidate::Directory(DirectoryState {
            directory: Directory::Home,
            exists: false,
            permissions_ok: false,
        }),
        Candidate::Directory(DirectoryState {
            directory: Directory::Runs,
            exists: true,
            permissions_ok: false,
        }),
    ]
}

fn cache_and_plugin_candidates() -> Vec<Candidate> {
    vec![
        Candidate::Cache(CacheState {
            cache: DisposableCache::Checkout,
            in_use: false,
        }),
        Candidate::PluginActivation(plugin_state("1.0.0", "1.0.0", false)),
    ]
}

fn recovery_candidates() -> Vec<Candidate> {
    vec![
        Candidate::Ownership(ownership_state(10, 20)),
        Candidate::Worktree(worktree_state(false, false)),
        Candidate::DerivedState(derived_state(true, false)),
    ]
}

fn unsafe_candidates() -> Vec<Candidate> {
    vec![
        Candidate::Cache(CacheState {
            cache: DisposableCache::Config,
            in_use: true,
        }),
        Candidate::PluginActivation(plugin_state("1.0.0", "1.0.0", true)),
        Candidate::PluginActivation(plugin_state("1.0.0", "2.0.0", false)),
        Candidate::Ownership(ownership_state(30, 20)),
        Candidate::Worktree(worktree_state(false, true)),
        Candidate::DerivedState(derived_state(true, true)),
        Candidate::DerivedState(derived_state(false, false)),
    ]
}

fn plugin_state(recorded: &str, installed: &str, active: bool) -> PluginActivationState {
    PluginActivationState {
        activation_id: "activation".to_owned(),
        run_id: "run-plugin".to_owned(),
        plugin: "bureau".to_owned(),
        recorded_version: recorded.to_owned(),
        installed_version: installed.to_owned(),
        stale: true,
        run_active: active,
    }
}

fn ownership_state(expires_at_ms: u64, observed_at_ms: u64) -> OwnershipState {
    OwnershipState {
        ownership: Ownership {
            assignment: "assignment".to_owned(),
            forge: "github".to_owned(),
            external_id: "42".to_owned(),
            run_id: "run-owner".to_owned(),
            owner_id: "owner".to_owned(),
            expires_at_ms,
        },
        observed_at_ms,
    }
}

fn worktree_state(run_exists: bool, ownership_active: bool) -> WorktreeState {
    WorktreeState {
        run_id: "run-worktree".to_owned(),
        run_exists,
        ownership_active,
    }
}

fn derived_state(history: bool, active: bool) -> DerivedState {
    DerivedState {
        run_id: format!("run-derived-{history}-{active}"),
        durable_history_exists: history,
        needs_rebuild: true,
        run_active: active,
    }
}
