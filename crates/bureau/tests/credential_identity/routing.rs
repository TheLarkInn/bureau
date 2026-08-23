//! No credential reaches a host its own repos never named.
//!
//! A run's work forge is the primary repo's; a context repo on another
//! host has its own credential and its own client. Sending every value
//! through the work forge would hand an Azure DevOps or GitHub
//! Enterprise secret to the wrong host, so each value is only ever
//! offered to the client a repo naming it points at.

use std::collections::BTreeMap;
use std::sync::Arc;

use bureau::contract::StepOutcome;
use bureau::engine::RunPlan;
use bureau::forge::fake::FakeForge;
use bureau::process::Secret;

use super::rig;

const WORK_VALUE: &str = "work-value";
const CONTEXT_VALUE: &str = "context-value";

/// One host per credential: the work forge answers for `git-main`, a
/// second forge for the context repo's `ado-main`.
struct Hosts {
    work: Arc<FakeForge>,
    context: Arc<FakeForge>,
}

fn hosts(rig: &rig::Rig) -> Hosts {
    let context = Arc::new(FakeForge::default());
    context.verify_identity_as("context-bot");
    rig.forge.verify_identity_as("work-bot");
    Hosts {
        work: rig.forge.clone(),
        context,
    }
}

/// A plan whose two credentials each name one authorized host, with the
/// identity each of them must authenticate as declared.
fn two_host_plan(rig: &rig::Rig, hosts: &Hosts) -> RunPlan {
    let mut plan = rig.plan(vec![rig::det_step("check", "true", Some("done"))]);
    plan.credentials = BTreeMap::from([
        ("git-main".to_owned(), Secret::new(WORK_VALUE)),
        ("ado-main".to_owned(), Secret::new(CONTEXT_VALUE)),
    ]);
    plan.identities = BTreeMap::from([
        ("git-main".to_owned(), "work-bot".to_owned()),
        ("ado-main".to_owned(), "context-bot".to_owned()),
    ]);
    plan.identity_forges = BTreeMap::from([
        (
            "git-main".to_owned(),
            vec![rig::authorized("github work", &hosts.work)],
        ),
        (
            "ado-main".to_owned(),
            vec![rig::authorized("ado context", &hosts.context)],
        ),
    ]);
    plan
}

/// Each value goes to its own host and to no other: the work forge is
/// never asked about the context repo's credential, and the run still
/// reaches its terminal.
#[tokio::test]
async fn each_credential_reaches_only_its_own_host() {
    let rig = rig::Rig::new();
    let hosts = hosts(&rig);
    let plan = two_host_plan(&rig, &hosts);
    let outcome = rig.engine().run(&plan).await;
    assert_eq!(
        (
            outcome.outcome,
            hosts.work.identified(),
            hosts.context.identified(),
        ),
        (
            StepOutcome::NoWork,
            vec![Secret::new(WORK_VALUE)],
            vec![Secret::new(CONTEXT_VALUE)],
        )
    );
}

/// The identities pinned in `run_started` come from the hosts that
/// answered, one per credential.
#[tokio::test]
async fn every_authorized_host_answers_for_its_own_credential() {
    let rig = rig::Rig::new();
    let hosts = hosts(&rig);
    let plan = two_host_plan(&rig, &hosts);
    let outcome = rig.engine().run(&plan).await;
    assert_eq!(
        (outcome.outcome, super::pinned(&rig, &plan.run_id)),
        (
            StepOutcome::NoWork,
            BTreeMap::from([
                ("ado-main".to_owned(), "context-bot".to_owned()),
                ("git-main".to_owned(), "work-bot".to_owned()),
            ])
        )
    );
}

/// A credential no registered repo names authorizes no host at all, so
/// a declared identity for it fails closed instead of being checked
/// against whichever client happened to be at hand.
#[tokio::test]
async fn a_credential_no_repo_names_is_never_sent_anywhere() {
    let rig = rig::Rig::new();
    let hosts = hosts(&rig);
    let mut plan = two_host_plan(&rig, &hosts);
    plan.identity_forges.remove("ado-main");
    let outcome = rig.engine().run(&plan).await;
    assert_eq!(
        (
            outcome.outcome,
            outcome.message.contains("ado-main"),
            hosts.context.identified(),
        ),
        (StepOutcome::Failure, true, Vec::new())
    );
}
