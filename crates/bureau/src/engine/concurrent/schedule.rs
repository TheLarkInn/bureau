//! Bounded member launch and stop-on-failure cancellation.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use tokio::task::JoinSet;

use crate::adapters::Execution;
use crate::config::{Completion, StepDef};
use crate::contract::StepOutcome;

use super::super::context::RunCtx;
use super::member;

pub(super) struct Schedule {
    pending: VecDeque<StepDef>,
    active: BTreeMap<String, PathBuf>,
    cancelled: BTreeSet<String>,
    tasks: JoinSet<(String, Execution)>,
    limit: usize,
    completion: Completion,
}

impl Schedule {
    pub(super) fn fill(
        &mut self,
        ctx: &RunCtx,
        mirror: &Path,
        root: &Path,
        snapshot: &str,
    ) -> Vec<String> {
        let mut started = Vec::new();
        while self.tasks.len() < self.limit {
            let Some(step) = self.pending.pop_front() else {
                break;
            };
            started.push(self.spawn(ctx, mirror, root, snapshot, step));
        }
        started
    }

    fn spawn(
        &mut self,
        ctx: &RunCtx,
        mirror: &Path,
        root: &Path,
        snapshot: &str,
        step: StepDef,
    ) -> String {
        let name = step.name.clone();
        self.active
            .insert(name.clone(), member::cancel_path(root, &name));
        let future = member::run(
            ctx.clone(),
            mirror.to_path_buf(),
            root.to_path_buf(),
            snapshot.to_owned(),
            step,
        );
        self.tasks.spawn(future);
        name
    }

    pub(super) async fn next(&mut self) -> Option<(String, Execution)> {
        match self.tasks.join_next().await? {
            Ok((name, execution)) => {
                self.active.remove(&name);
                Some((name, execution))
            }
            Err(error) => Some((
                "concurrent-runtime".to_owned(),
                member::failed(&format!("concurrent member task failed: {error}")),
            )),
        }
    }

    pub(super) fn cancel_after_failure(
        &mut self,
        outcome: StepOutcome,
        halted: bool,
        reason: &str,
    ) -> Vec<String> {
        let stop = halted
            || (self.completion == Completion::StopOnFailure && outcome == StepOutcome::Failure);
        if !stop {
            return Vec::new();
        }
        let mut names: Vec<_> = self.pending.drain(..).map(|step| step.name).collect();
        names.extend(self.active.keys().cloned());
        for name in &names {
            self.cancelled.insert(name.clone());
        }
        for path in self.active.values() {
            let _ = std::fs::write(path, reason);
        }
        names
    }

    pub(super) fn was_cancelled(&mut self, name: &str) -> bool {
        self.cancelled.remove(name)
    }

    pub(super) fn is_finished(&self) -> bool {
        self.pending.is_empty() && self.tasks.is_empty()
    }
}

/// A fresh schedule over `members` with `limit` parallel slots and the
/// group's completion rule.
pub(super) fn new(members: Vec<StepDef>, limit: usize, completion: Completion) -> Schedule {
    Schedule {
        pending: members.into(),
        active: BTreeMap::new(),
        cancelled: BTreeSet::new(),
        tasks: JoinSet::new(),
        limit,
        completion,
    }
}
