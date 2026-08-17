//! Conservative planning and confirmed execution of reversible repairs.

mod local;

pub use bureau_lifecycle::repair::{
    Action, CacheState, Candidate, Confirmation, DerivedState, DirectoryState, DisposableCache,
    Effects, Error, Machine, Ownership, OwnershipState, Plan, PluginActivationState, SkipReason,
    Skipped, State, Summary, WorktreeState, plan, run,
};
pub use local::LocalEffects;
