use super::model::{
    Action, CacheState, Candidate, DerivedState, DirectoryState, OwnershipState, Plan,
    PluginActivationState, SkipReason, Skipped, WorktreeState,
};

fn skip(skipped: &mut Vec<Skipped>, target: String, reason: SkipReason) {
    skipped.push(Skipped { target, reason });
}

fn plugin_action(state: PluginActivationState) -> Action {
    Action::RestorePluginActivation {
        run_id: state.run_id,
        activation_id: state.activation_id,
        plugin: state.plugin,
        version: state.installed_version,
    }
}

fn assess_directory(state: DirectoryState, actions: &mut Vec<Action>) {
    if !state.exists {
        actions.push(Action::CreateDirectory {
            directory: state.directory,
        });
    } else if !state.permissions_ok {
        actions.push(Action::FixDirectoryPermissions {
            directory: state.directory,
        });
    }
}

fn assess_cache(state: CacheState, actions: &mut Vec<Action>, skipped: &mut Vec<Skipped>) {
    if state.in_use {
        skip(
            skipped,
            format!("{:?} cache", state.cache),
            SkipReason::LiveWork,
        );
    } else {
        actions.push(Action::ClearCache { cache: state.cache });
    }
}

fn assess_plugin(
    state: PluginActivationState,
    actions: &mut Vec<Action>,
    skipped: &mut Vec<Skipped>,
) {
    if !state.stale {
        return;
    }
    let target = format!(
        "run {} activation {} plugin {}",
        state.run_id, state.activation_id, state.plugin
    );
    if state.run_active {
        skip(skipped, target, SkipReason::LiveWork);
        return;
    }
    if state.recorded_version != state.installed_version {
        skip(skipped, target, SkipReason::PluginVersionChanged);
        return;
    }
    actions.push(plugin_action(state));
}

fn assess_ownership(state: OwnershipState, actions: &mut Vec<Action>, skipped: &mut Vec<Skipped>) {
    if state.ownership.expires_at_ms <= state.observed_at_ms {
        actions.push(Action::ReapExpiredOwnership {
            ownership: state.ownership,
        });
    } else {
        let target = format!("run {} ownership", state.ownership.run_id);
        skip(skipped, target, SkipReason::OwnershipNotExpired);
    }
}

fn assess_worktree(state: WorktreeState, actions: &mut Vec<Action>, skipped: &mut Vec<Skipped>) {
    if state.ownership_active {
        skip(
            skipped,
            format!("run {} worktree", state.run_id),
            SkipReason::LiveWork,
        );
    } else if !state.run_exists {
        actions.push(Action::PruneOrphanWorktree {
            run_id: state.run_id,
        });
    }
}

fn assess_derived(state: DerivedState, actions: &mut Vec<Action>, skipped: &mut Vec<Skipped>) {
    if !state.needs_rebuild {
        return;
    }
    if state.run_active {
        skip(skipped, state.run_id, SkipReason::LiveWork);
        return;
    }
    if !state.durable_history_exists {
        skip(skipped, state.run_id, SkipReason::DurableHistoryUnavailable);
        return;
    }
    actions.push(Action::RebuildDerivedState {
        run_id: state.run_id,
    });
}

fn assess(candidate: Candidate, actions: &mut Vec<Action>, skipped: &mut Vec<Skipped>) {
    match candidate {
        Candidate::Directory(state) => assess_directory(state, actions),
        Candidate::Cache(state) => assess_cache(state, actions, skipped),
        Candidate::PluginActivation(state) => assess_plugin(state, actions, skipped),
        Candidate::Ownership(state) => assess_ownership(state, actions, skipped),
        Candidate::Worktree(state) => assess_worktree(state, actions, skipped),
        Candidate::DerivedState(state) => assess_derived(state, actions, skipped),
    }
}

/// Builds a canonical plan containing only reversible, currently safe repairs.
#[must_use]
pub fn plan(candidates: impl IntoIterator<Item = Candidate>) -> Plan {
    let mut actions = Vec::new();
    let mut skipped = Vec::new();
    for candidate in candidates {
        assess(candidate, &mut actions, &mut skipped);
    }
    Plan::new(actions, skipped)
}
