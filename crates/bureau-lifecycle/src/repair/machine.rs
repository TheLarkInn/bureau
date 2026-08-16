use crate::home::Directory;

use super::model::{Action, DisposableCache, Ownership, Plan};

/// Explicit API response required before any repair effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmation {
    /// Apply the previewed plan.
    Approve,
    /// Perform no effects.
    Decline,
}

/// Repair execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Waiting for the explicit confirmation response.
    AwaitingConfirmation,
    /// Applying canonical actions in order.
    Applying,
    /// Every planned action succeeded.
    Complete,
    /// Confirmation was declined without effects.
    Declined,
    /// An effect failed and execution stopped.
    Failed,
}

/// Mutable effects limited to the seven reversible repair operations.
pub trait Effects {
    /// Creates a fixed expected directory.
    ///
    /// # Errors
    /// Returns a local filesystem failure.
    fn create_directory(&mut self, directory: Directory) -> Result<(), String>;
    /// Restores expected permissions on a fixed directory.
    ///
    /// # Errors
    /// Returns a local filesystem failure.
    fn fix_directory_permissions(&mut self, directory: Directory) -> Result<(), String>;
    /// Clears one disposable cache after rechecking it is unused.
    ///
    /// # Errors
    /// Returns a failure or reports that the cache became live.
    fn clear_cache(&mut self, cache: DisposableCache) -> Result<(), String>;
    /// Restores stale activation only when `version` remains installed.
    ///
    /// # Errors
    /// Returns a failure or reports changed plugin or live-run state.
    fn restore_plugin_activation(
        &mut self,
        run_id: &str,
        activation_id: &str,
        plugin: &str,
        version: &str,
    ) -> Result<(), String>;
    /// Deletes only the still-matching expired ownership record.
    ///
    /// # Errors
    /// Returns a failure or reports that ownership changed or became live.
    fn reap_expired_ownership(&mut self, ownership: &Ownership) -> Result<(), String>;
    /// Removes only an unowned worktree whose durable run is absent.
    ///
    /// # Errors
    /// Returns a failure or reports that the worktree became owned.
    fn prune_orphan_worktree(&mut self, run_id: &str) -> Result<(), String>;
    /// Replays durable events without modifying them.
    ///
    /// # Errors
    /// Returns a replay or derived-state write failure.
    fn rebuild_derived_state(&mut self, run_id: &str) -> Result<(), String>;
}

/// Invalid or failed repair execution.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// An action was requested before explicit approval.
    #[error("repair requires explicit confirmation")]
    ConfirmationRequired,
    /// An operation is not valid in the current state.
    #[error("repair operation is invalid while state is {0:?}")]
    InvalidState(State),
    /// A reversible effect failed and execution stopped.
    #[error("repair effect failed for {action:?}: {message}")]
    Effect {
        /// Action that failed.
        action: Box<Action>,
        /// Effect-provided failure.
        message: String,
    },
}

/// Final execution summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    /// Terminal state.
    pub state: State,
    /// Number of successfully applied actions.
    pub applied: usize,
}

/// Explicitly confirmed, deterministic repair execution.
#[derive(Debug)]
pub struct Machine {
    plan: Plan,
    state: State,
    next: usize,
}

impl Machine {
    /// Creates a machine that cannot execute before confirmation.
    #[must_use]
    pub const fn new(plan: Plan) -> Self {
        Self {
            plan,
            state: State::AwaitingConfirmation,
            next: 0,
        }
    }

    /// Current state.
    #[must_use]
    pub const fn state(&self) -> State {
        self.state
    }

    /// Previewed plan.
    #[must_use]
    pub const fn plan(&self) -> &Plan {
        &self.plan
    }

    /// Supplies the one explicit confirmation response.
    ///
    /// # Errors
    /// Rejects confirmation outside the initial state.
    pub fn confirm(&mut self, confirmation: Confirmation) -> Result<(), Error> {
        if self.state != State::AwaitingConfirmation {
            return Err(Error::InvalidState(self.state));
        }
        self.state = self.confirmed_state(confirmation);
        Ok(())
    }

    /// Next approved action, if any.
    ///
    /// # Errors
    /// Rejects access before confirmation or after failure.
    pub fn next_action(&self) -> Result<Option<&Action>, Error> {
        match self.state {
            State::AwaitingConfirmation => Err(Error::ConfirmationRequired),
            State::Failed => Err(Error::InvalidState(State::Failed)),
            State::Applying => Ok(self.plan.actions().get(self.next)),
            State::Complete | State::Declined => Ok(None),
        }
    }

    /// Applies at most one approved action.
    ///
    /// # Errors
    /// Rejects unconfirmed execution and stops on an effect failure.
    pub fn apply_next(&mut self, effects: &mut impl Effects) -> Result<bool, Error> {
        let Some(action) = self.next_action()?.cloned() else {
            return Ok(false);
        };
        if let Err(message) = apply(effects, &action) {
            self.state = State::Failed;
            return Err(Error::Effect {
                action: Box::new(action),
                message,
            });
        }
        self.next += 1;
        self.complete_if_finished();
        Ok(true)
    }

    /// Returns the summary after completion or decline.
    ///
    /// # Errors
    /// Rejects nonterminal states.
    pub fn finish(self) -> Result<Summary, Error> {
        match self.state {
            State::Complete | State::Declined => Ok(Summary {
                state: self.state,
                applied: self.next,
            }),
            state => Err(Error::InvalidState(state)),
        }
    }

    fn confirmed_state(&self, confirmation: Confirmation) -> State {
        match confirmation {
            Confirmation::Decline => State::Declined,
            Confirmation::Approve if self.plan.is_empty() => State::Complete,
            Confirmation::Approve => State::Applying,
        }
    }

    fn complete_if_finished(&mut self) {
        if self.next == self.plan.actions().len() {
            self.state = State::Complete;
        }
    }
}

/// Confirms or declines a plan and drives it to a terminal state.
///
/// # Errors
/// Propagates invalid-state and effect failures.
pub fn run(
    plan: Plan,
    confirmation: Confirmation,
    effects: &mut impl Effects,
) -> Result<Summary, Error> {
    let mut machine = Machine::new(plan);
    machine.confirm(confirmation)?;
    while machine.apply_next(effects)? {}
    machine.finish()
}

fn apply(effects: &mut impl Effects, action: &Action) -> Result<(), String> {
    match action {
        Action::CreateDirectory { directory } => effects.create_directory(*directory),
        Action::FixDirectoryPermissions { directory } => {
            effects.fix_directory_permissions(*directory)
        }
        Action::ClearCache { cache } => effects.clear_cache(*cache),
        Action::RestorePluginActivation {
            run_id,
            activation_id,
            plugin,
            version,
        } => effects.restore_plugin_activation(run_id, activation_id, plugin, version),
        Action::ReapExpiredOwnership { ownership } => effects.reap_expired_ownership(ownership),
        Action::PruneOrphanWorktree { run_id } => effects.prune_orphan_worktree(run_id),
        Action::RebuildDerivedState { run_id } => effects.rebuild_derived_state(run_id),
    }
}
