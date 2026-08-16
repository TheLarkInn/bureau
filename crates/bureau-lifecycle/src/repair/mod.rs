//! Conservative planning and confirmed execution of reversible repairs.

mod machine;
mod model;
mod plan;

pub use machine::{Confirmation, Effects, Error, Machine, State, Summary, run};
pub use model::{
    Action, CacheState, Candidate, DerivedState, DirectoryState, DisposableCache, Ownership,
    OwnershipState, Plan, PluginActivationState, SkipReason, Skipped, WorktreeState,
};
pub use plan::plan;
