//! Run deadline, live approval, and idempotent terminal recovery.

#[path = "engine/rig.rs"]
mod rig;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use bureau::contract::{StepOutcome, Trust};
use bureau::forge::fake::FakeForge;
use bureau::forge::{Error, Forge, Item, Pr, PrRequest, PrStatus};
use bureau::runlog::{self, EventKind};

#[tokio::test]
async fn zero_hour_deadline_escalates_before_a_step_spawns() {
    let rig = rig::Rig::new();
    use_fixture_helpers(&rig);
    let mut plan = rig.plan(vec![rig::det_step("never", "exit 99", Some("done"))]);
    plan.assignment.limits.max_run_hours = Some(0);
    let outcome = rig.engine().run(&plan).await;
    assert!(
        outcome.outcome == StepOutcome::Blocked && outcome.message.contains("0 hour deadline"),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn removing_approval_blocks_the_next_step() {
    let rig = rig::Rig::new();
    let labels = vec!["agent-approved".to_owned()];
    rig.forge.set_labels("42", &labels).await.expect("approve");
    let steps = vec![
        rig::det_step(
            "first",
            "sleep 0.4; echo changed >> file.txt",
            Some("second"),
        ),
        rig::det_step("second", "echo should-not-run >> file.txt", Some("done")),
    ];
    let mut plan = rig.plan(steps);
    plan.assignment.work.approval_label = Some("agent-approved".to_owned());
    let engine = rig.engine();
    let task = tokio::spawn(async move { engine.run(&plan).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    rig.forge.set_labels("42", &[]).await.expect("revoke");
    let outcome = task.await.expect("run task");
    assert_eq!(outcome.outcome, StepOutcome::Blocked, "{outcome:?}");
}

#[tokio::test]
async fn crash_after_pr_creation_adopts_the_existing_pr() {
    let rig = rig::Rig::new();
    let plan = rig.plan(vec![rig::det_step(
        "edit",
        "echo changed >> file.txt",
        Some("done"),
    )]);
    let first = rig.engine().run(&plan).await;
    remove_finished_event(rig.dir.path(), &plan.run_id);
    let second = rig.engine().run(&plan).await;
    let prs = rig
        .forge
        .open_prs(&rig.url, "bureau/")
        .await
        .expect("open prs");
    assert_eq!(
        (first.outcome, second.outcome, prs.len()),
        (StepOutcome::Success, StepOutcome::Success, 1)
    );
}

fn remove_finished_event(root: &std::path::Path, run_id: &str) {
    let dir = root.join("runs").join(run_id);
    let events = runlog::read_events(&dir).expect("events");
    let kept: Vec<_> = events
        .into_iter()
        .filter(|event| event.kind != EventKind::RunFinished)
        .collect();
    let mut bytes = Vec::new();
    for event in kept {
        bytes.extend(serde_json::to_vec(&event).expect("event"));
        bytes.push(b'\n');
    }
    std::fs::write(dir.join(runlog::EVENTS_FILE), bytes).expect("rewrite log");
}

fn use_fixture_helpers(rig: &rig::Rig) {
    let result = rig::result(StepOutcome::Success, "unused");
    let fixture = rig::fixture(rig.dir.path(), "unused.json", &result);
    let _ = (
        rig::step("unused", bureau::config::StepKind::Deterministic),
        rig::agent_step("unused", &fixture, None),
        rig::decision_step("unused", "other"),
        Trust::Derived,
    );
}

const READY: &str = "bureau:maintenance-ready";
const REPORTED: &str = "bureau:maintenance-reported";

fn labels(names: &[&str]) -> Vec<String> {
    names.iter().map(|&name| name.to_owned()).collect()
}

/// What the forge reports after the first step, before its boundary check.
enum Change {
    Labels(Vec<String>, Vec<String>),
    Fail,
    Vanish,
}

/// The fake forge behind an admission filter excluding `REPORTED`, as the
/// maintenance assignments' filters do. Its second read applies `change`.
struct MidRun {
    inner: Arc<FakeForge>,
    change: Change,
    reads: AtomicUsize,
}

impl MidRun {
    async fn observe(&self) -> Result<(), Error> {
        match (&self.change, self.reads.fetch_add(1, Ordering::SeqCst)) {
            (_, 0) => Ok(()),
            (Change::Fail, _) => Err(Error::Parse("offline forge outage".to_owned())),
            (Change::Labels(add, remove), 1) => self.inner.update_labels("42", add, remove).await,
            (Change::Vanish, 1) => {
                self.inner.remove_item("42");
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[async_trait]
impl Forge for MidRun {
    async fn query(&self, source: &str, filter: &str) -> Result<Vec<Item>, Error> {
        self.observe().await?;
        let mut items = self.inner.query(source, filter).await?;
        items.retain(|item| !item.labels.iter().any(|label| label == REPORTED));
        Ok(items)
    }

    async fn item(&self, item_id: &str) -> Result<Item, Error> {
        self.observe().await?;
        self.inner.item(item_id).await
    }

    async fn open_prs(&self, repo: &str, prefix: &str) -> Result<Vec<Pr>, Error> {
        self.inner.open_prs(repo, prefix).await
    }

    async fn create_pr(&self, request: &PrRequest) -> Result<Pr, Error> {
        self.inner.create_pr(request).await
    }

    async fn pr_status(&self, repo: &str, number: u64) -> Result<PrStatus, Error> {
        self.inner.pr_status(repo, number).await
    }

    async fn comment(&self, item_id: &str, body: &str) -> Result<(), Error> {
        self.inner.comment(item_id, body).await
    }

    async fn set_labels(&self, item_id: &str, labels: &[String]) -> Result<(), Error> {
        self.inner.set_labels(item_id, labels).await
    }

    async fn update_labels(
        &self,
        item_id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<(), Error> {
        self.inner.update_labels(item_id, add, remove).await
    }
}

/// One approved run whose forge state changes during its only step.
async fn run_with(change: Change) -> (StepOutcome, String) {
    let rig = rig::Rig::new();
    rig.forge
        .set_labels("42", &labels(&[READY]))
        .await
        .expect("approve");
    let step = rig::det_step("report", "echo changed >> file.txt", Some("done"));
    let mut plan = rig.plan(vec![step]);
    plan.assignment.work.approval_label = Some(READY.to_owned());
    plan.forge = Arc::new(MidRun {
        inner: rig.forge.clone(),
        change,
        reads: AtomicUsize::new(0),
    });
    let outcome = rig.engine().run(&plan).await;
    (outcome.outcome, outcome.message)
}

#[tokio::test]
async fn approval_recheck_reads_the_item_not_the_admission_filter() {
    let cases = [
        (
            Change::Labels(labels(&[REPORTED]), Vec::new()),
            StepOutcome::Success,
            "",
        ),
        (
            Change::Labels(Vec::new(), labels(&[READY])),
            StepOutcome::Blocked,
            "is missing",
        ),
        (Change::Fail, StepOutcome::Blocked, "offline forge outage"),
        (Change::Vanish, StepOutcome::Blocked, "`42` not found"),
    ];
    let mut seen = Vec::new();
    for (change, outcome, needle) in cases {
        let (got, message) = run_with(change).await;
        seen.push((got == outcome && message.contains(needle), got, message));
    }
    assert!(seen.iter().all(|case| case.0), "{seen:#?}");
}
