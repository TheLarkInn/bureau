use serde::{Deserialize, Serialize};

use crate::home::Directory;

/// A disposable local cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisposableCache {
    /// Bare checkout mirrors.
    Checkout,
    /// Committed config checkouts.
    Config,
}

/// Observed state for one expected directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectoryState {
    /// Fixed layout directory.
    pub directory: Directory,
    /// Whether it currently exists as a directory.
    pub exists: bool,
    /// Whether its permissions match local expectations.
    pub permissions_ok: bool,
}

/// Observed state for one disposable cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheState {
    /// Cache eligible for clearing.
    pub cache: DisposableCache,
    /// Whether live work currently uses it.
    pub in_use: bool,
}

/// Observed stale temporary plugin activation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginActivationState {
    /// Stable identity of one activation record within the run.
    pub activation_id: String,
    /// Durable run that recorded the activation.
    pub run_id: String,
    /// Plugin name, never plugin contents.
    pub plugin: String,
    /// Version recorded before temporary activation.
    pub recorded_version: String,
    /// Currently installed version.
    pub installed_version: String,
    /// Whether temporary activation remains on disk.
    pub stale: bool,
    /// Whether its run is live.
    pub run_active: bool,
}

/// Exact durable ownership identity used for guarded expiry reaping.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Ownership {
    /// Assignment holding the ownership record.
    pub assignment: String,
    /// Forge holding the work item.
    pub forge: String,
    /// Forge work item identifier.
    pub external_id: String,
    /// Durable run identifier.
    pub run_id: String,
    /// Supervisor generation identifier.
    pub owner_id: String,
    /// Observed expiry in Unix epoch milliseconds.
    pub expires_at_ms: u64,
}

/// Observed ownership and the observation time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipState {
    /// Exact record to compare and reap.
    pub ownership: Ownership,
    /// Observation time in Unix epoch milliseconds.
    pub observed_at_ms: u64,
}

/// Observed worktree registration and ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeState {
    /// Run identifier used to locate the expected worktree.
    pub run_id: String,
    /// Whether its durable run directory still exists.
    pub run_exists: bool,
    /// Whether a live ownership record protects it.
    pub ownership_active: bool,
}

/// Observed derived run state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedState {
    /// Durable run whose derived state may be replayed.
    pub run_id: String,
    /// Whether its append-only event history is available.
    pub durable_history_exists: bool,
    /// Whether a rebuild is needed.
    pub needs_rebuild: bool,
    /// Whether the run is live.
    pub run_active: bool,
}

/// One observed condition from which a safe plan can be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Candidate {
    /// Expected directory state.
    Directory(DirectoryState),
    /// Explicit request to clear one disposable cache.
    Cache(CacheState),
    /// Stale temporary plugin activation.
    PluginActivation(PluginActivationState),
    /// Possibly expired ownership.
    Ownership(OwnershipState),
    /// Possibly orphaned worktree.
    Worktree(WorktreeState),
    /// Possibly stale derived run state.
    DerivedState(DerivedState),
}

/// One reversible repair. No variant can change policy, credentials, plugin
/// versions, live work, or durable event history.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Create one fixed expected directory.
    CreateDirectory {
        /// Directory to create.
        directory: Directory,
    },
    /// Restore expected permissions on one fixed directory.
    FixDirectoryPermissions {
        /// Directory whose permissions are repaired.
        directory: Directory,
    },
    /// Clear one typed disposable cache.
    ClearCache {
        /// Cache to clear.
        cache: DisposableCache,
    },
    /// Restore temporary activation using the already installed version.
    RestorePluginActivation {
        /// Durable run that owns the restoration record.
        run_id: String,
        /// Stable activation record identity.
        activation_id: String,
        /// Plugin name.
        plugin: String,
        /// Unchanged installed and recorded version.
        version: String,
    },
    /// Reap one still-matching expired ownership record.
    ReapExpiredOwnership {
        /// Exact observed identity and expiry used by the guarded effect.
        ownership: Ownership,
    },
    /// Prune an unowned worktree with no durable run directory.
    PruneOrphanWorktree {
        /// Orphaned worktree's former run identifier.
        run_id: String,
    },
    /// Replay append-only events to rebuild derived run state.
    RebuildDerivedState {
        /// Durable run identifier.
        run_id: String,
    },
}

/// Reason an observed candidate was conservatively excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// A live run or ownership record protects the target.
    LiveWork,
    /// Restoring would require changing plugin versions.
    PluginVersionChanged,
    /// Ownership has not expired.
    OwnershipNotExpired,
    /// Rebuilding cannot replay intact durable history.
    DurableHistoryUnavailable,
}

/// One candidate intentionally excluded from execution.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Skipped {
    /// Stable non-secret target description.
    pub target: String,
    /// Safety reason.
    pub reason: SkipReason,
}

/// Canonical repair preview produced only by the conservative planner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    actions: Vec<Action>,
    skipped: Vec<Skipped>,
}

impl Plan {
    pub(super) fn new(mut actions: Vec<Action>, mut skipped: Vec<Skipped>) -> Self {
        actions.sort();
        actions.dedup();
        skipped.sort();
        skipped.dedup();
        Self { actions, skipped }
    }

    /// Reversible actions in canonical order.
    #[must_use]
    pub fn actions(&self) -> &[Action] {
        &self.actions
    }

    /// Candidates excluded by safety checks.
    #[must_use]
    pub fn skipped(&self) -> &[Skipped] {
        &self.skipped
    }

    /// Whether confirmation would perform no effects.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}
