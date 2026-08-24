//! Credential identity: the pre-spawn check that a resolved value both
//! works and belongs to the expected account (DESIGN.md section 7).
//!
//! Offline throughout: the fake forge answers nothing about identity
//! until a test opts in, which is exactly the rule being pinned here.

#[path = "engine/rig.rs"]
mod rig;

#[path = "credential_identity/routing.rs"]
mod routing;

#[path = "credential_identity/resumed.rs"]
mod resumed;

use std::collections::BTreeMap;

use bureau::config::validate_identities;
use bureau::config::{Access, Config, ForgeKind, Repo};
use bureau::contract::StepOutcome;
use bureau::engine::RunPlan;
use bureau::forge::fake::FakeForge;
use bureau::forge::identity::{Check, Expected, IdentityError, Reported, verify};
use bureau::process::Secret;
use bureau::runlog::{self, EventKind, RunStartedData};
use bureau::setup::Settings;

const DECLARED: &str = "bureau-bot";

/// Two declared credentials: one names the identity it must be, the
/// other omits the field entirely.
const SETTINGS_YAML: &str = concat!(
    "config:\n  kind: single_repository\n",
    "  remote: https://example.invalid/work.git\n  reference: main\n",
    "credentials:\n",
    "  git-main:\n    source: environment\n",
    "    variable: BUREAU_CREDENTIAL_GIT_MAIN\n    identity: bureau-bot\n",
    "  other:\n    source: file\n    path: /run/credentials/other\n",
);

/// One credential to check against the fake, as a run would.
const fn check<'a>(credential: &'a Secret, expected: Option<&'a str>) -> Check<'a> {
    Check {
        reference: "git-main",
        credential,
        expected,
        expectation: Expected::Declared,
    }
}

/// One registry repo referencing `credential`.
fn config_with_credential(credential: &str) -> Config {
    let repo = Repo {
        url: "https://example.invalid/work.git".to_owned(),
        forge: ForgeKind::Github,
        access: Access::Push,
        credential: credential.to_owned(),
    };
    Config {
        repos: BTreeMap::from([("main".to_owned(), repo)]),
        roles: BTreeMap::new(),
        assignments: BTreeMap::new(),
        label_rules: BTreeMap::new(),
        pipelines: BTreeMap::new(),
    }
}

/// Rig helpers this file does not exercise; the module is shared with
/// the engine suites, and unused fixtures would be a dead-code warning.
fn unused_rig_helpers(rig: &rig::Rig) {
    let _unused = (
        rig::step("unused", bureau::config::StepKind::Deterministic),
        rig::agent_step("unused", "fixture", None),
        rig::decision_step("unused", "other"),
        rig::result(StepOutcome::Success, "unused"),
        rig::fixture(
            rig.dir.path(),
            "unused.json",
            &rig::result(StepOutcome::Success, "unused"),
        ),
    );
}

/// A plan for one trivial step, declaring `git-main` must be `DECLARED`.
fn declaring_plan(rig: &rig::Rig) -> RunPlan {
    let mut plan = rig.plan(vec![rig::det_step("check", "true", Some("done"))]);
    plan.identities = BTreeMap::from([("git-main".to_owned(), DECLARED.to_owned())]);
    plan
}

/// The run's recorded events.
fn events(rig: &rig::Rig, run_id: &str) -> Vec<runlog::Event> {
    let dir = rig.dir.path().join("runs").join(run_id);
    runlog::read_events(&dir).expect("events read")
}

/// The identities pinned in the run's `run_started` payload.
fn pinned(rig: &rig::Rig, run_id: &str) -> BTreeMap<String, String> {
    events(rig, run_id)
        .iter()
        .find(|event| event.kind == EventKind::RunStarted)
        .and_then(|event| serde_json::from_value::<RunStartedData>(event.data.clone()).ok())
        .map(|started| started.identities)
        .unwrap_or_default()
}

fn started_steps(rig: &rig::Rig, run_id: &str) -> usize {
    events(rig, run_id)
        .iter()
        .filter(|event| event.kind == EventKind::StepStarted)
        .count()
}

/// A credential that is valid but belongs to another account fails the
/// run before any step spawns, naming reference, expected, and observed.
#[tokio::test]
async fn a_wrong_identity_aborts_the_run_before_any_step() {
    let rig = rig::Rig::new();
    unused_rig_helpers(&rig);
    rig.forge.verify_identity_as("someone-else");
    let plan = declaring_plan(&rig);
    let outcome = rig.engine().run(&plan).await;
    let message = outcome.message;
    assert_eq!(
        (
            outcome.outcome,
            started_steps(&rig, &plan.run_id),
            message.contains("git-main") && message.contains(DECLARED),
            message.contains("someone-else") && !message.contains("test-credential"),
        ),
        (StepOutcome::Failure, 0, true, true)
    );
}

/// A credential the forge refuses reads as invalid or expired, not as a
/// wrong identity — the two failures stay distinguishable.
#[tokio::test]
async fn a_refused_credential_reads_as_invalid_or_expired() {
    let rig = rig::Rig::new();
    rig.forge.reject_identity("bad credentials");
    let plan = declaring_plan(&rig);
    let outcome = rig.engine().run(&plan).await;
    assert_eq!(
        (
            outcome.outcome,
            started_steps(&rig, &plan.run_id),
            outcome.message.contains("invalid or expired"),
            outcome.message.contains("not the declared identity"),
        ),
        (StepOutcome::Failure, 0, true, false)
    );
}

/// Offline by default: the fake answers nothing about identity unless a
/// test opts in, so a declared identity alone never blocks a run and
/// nothing is pinned.
#[tokio::test]
async fn the_fake_forge_skips_verification_until_a_test_opts_in() {
    let rig = rig::Rig::new();
    let plan = declaring_plan(&rig);
    let outcome = rig.engine().run(&plan).await;
    assert_eq!(
        (outcome.outcome, pinned(&rig, &plan.run_id)),
        (StepOutcome::NoWork, BTreeMap::new())
    );
}

/// A forge that accepts a value without naming an account — a GitHub
/// App installation token — proves the value works and nothing more:
/// permissive without a declaration, unverifiable with one.
#[tokio::test]
async fn an_unnamed_acceptance_cannot_satisfy_a_declared_identity() {
    let forge = FakeForge::default();
    forge.accept_identity_unnamed();
    let credential = Secret::new("value");
    let permissive = verify(&forge, &check(&credential, None)).await;
    let declared = verify(&forge, &check(&credential, Some(DECLARED))).await;
    assert_eq!(
        (
            permissive.ok(),
            matches!(declared, Err(IdentityError::Unverifiable { .. })),
        ),
        (Some(Reported::Unnamed), true)
    );
}

/// Omitting `identity` keeps the credential permissive: the value is
/// verified as valid and matched against no name.
#[tokio::test]
async fn an_undeclared_identity_is_verified_but_not_matched() {
    let forge = FakeForge::default();
    forge.verify_identity_as("whoever");
    let credential = Secret::new("value");
    let verified = verify(&forge, &check(&credential, None))
        .await
        .expect("permissive verification");
    assert_eq!(
        verified,
        Reported::Account(bureau::forge::Identity::new("whoever"))
    );
}

/// Case is not identity: forge account names compare case-insensitively.
#[tokio::test]
async fn a_declared_identity_matches_regardless_of_case() {
    let forge = FakeForge::default();
    forge.verify_identity_as("Bureau-Bot");
    let credential = Secret::new("value");
    let matched = verify(&forge, &check(&credential, Some(DECLARED))).await;
    let mismatched = verify(&forge, &check(&credential, Some("other"))).await;
    assert_eq!(
        (
            matched.expect("match"),
            matches!(mismatched, Err(IdentityError::Mismatch { .. })),
        ),
        (
            Reported::Account(bureau::forge::Identity::new("Bureau-Bot")),
            true
        )
    );
}

/// The declaration lives on the credential in `settings.yaml`, beside
/// its source, and the field is optional: omitting it declares nothing.
#[test]
fn settings_declare_identity_per_credential_and_omission_declares_none() {
    let settings: Settings = serde_yaml_ng::from_str(SETTINGS_YAML).expect("settings parse");
    assert_eq!(
        (settings.credentials.len(), settings.declared_identities(),),
        (
            2,
            BTreeMap::from([("git-main".to_owned(), DECLARED.to_owned())])
        )
    );
}

/// An identity declared for a credential no repo references is reported
/// against `settings.yaml`, in the accumulate-all validation pass. The
/// reserved `config` reference is exempt and not unenforced: the runner
/// verifies it against the config remote before it fetches, and
/// `doctor` re-checks it read-only.
#[test]
fn validate_reports_an_identity_no_repo_references() {
    let config = config_with_credential("git-main");
    let declared = BTreeMap::from([
        ("git-main".to_owned(), DECLARED.to_owned()),
        ("config".to_owned(), DECLARED.to_owned()),
        ("unused".to_owned(), DECLARED.to_owned()),
    ]);
    let errors = validate_identities(&config, &declared);
    let reported: Vec<_> = errors
        .iter()
        .map(|error| (error.path.display().to_string(), error.message.clone()))
        .collect();
    assert_eq!(
        reported,
        vec![(
            "settings.yaml".to_owned(),
            "credential `unused`: `identity` is declared but no repo references this credential"
                .to_owned()
        )]
    );
}
