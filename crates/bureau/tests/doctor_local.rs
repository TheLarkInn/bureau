//! Offline production-effect tests for local doctor diagnostics.

pub mod local_effects_support;

use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::fs::PermissionsExt as _;

use bureau::doctor::{self, Area, CredentialIdentity, LocalEffects, Status};
use bureau::setup::{Credential, CredentialSource};

use local_effects_support::{Fixture, REPO_URL, SECRET_TEXT};

/// The config remote the fixture's settings name.
const CONFIG_REMOTE: &str = "https://example.invalid/config.git";

#[test]
fn local_doctor_inspects_all_areas_without_exposing_credentials() {
    let fixture = healthy_fixture("doctor-healthy");
    let effects = LocalEffects::new(fixture.layout())
        .search_path(fixture.bin.as_os_str())
        .identities(vec![CredentialIdentity::verified("work", "bureau-bot")]);
    let report = doctor::run(&effects);
    let statuses: Vec<_> = report
        .diagnostics()
        .iter()
        .map(|diagnostic| (diagnostic.area, diagnostic.status))
        .collect();
    let output = format!("{}\n{}", report.human(), report.json().expect("json"));
    assert_eq!(
        (statuses, output.contains(SECRET_TEXT)),
        (Area::ALL.map(|area| (area, Status::Ok)).to_vec(), false)
    );
}

/// Identity verification is the caller's read-only job: with nothing
/// injected the area says so, and injected results are reported per
/// credential, wrong-identity detail included.
#[test]
fn credential_identity_reports_injected_results_per_reference() {
    let fixture = healthy_fixture("doctor-identity");
    let unverified = LocalEffects::new(fixture.layout()).search_path(fixture.bin.as_os_str());
    let mismatched = LocalEffects::new(fixture.layout())
        .search_path(fixture.bin.as_os_str())
        .identities(vec![
            CredentialIdentity::verified("work", "bureau-bot"),
            CredentialIdentity::failed("other", "authenticates as `someone-else`"),
        ]);
    let silent = diagnostic(&doctor::run(&unverified), Area::CredentialIdentity).clone();
    let report = doctor::run(&mismatched);
    let reported = diagnostic(&report, Area::CredentialIdentity);
    assert_eq!(
        (
            silent.status,
            reported.status,
            reported
                .message
                .contains("work: authenticates as `bureau-bot`"),
            reported.message.contains("someone-else")
        ),
        (Status::Warning, Status::Error, true, true)
    );
}

#[test]
fn recovery_replay_is_read_only_even_with_a_torn_final_line() {
    let fixture = healthy_fixture("doctor-replay");
    let run = fixture.layout().runs().join("finished");
    fs::write(run.join("state.json"), b"not derived state").expect("stale state");
    fs::OpenOptions::new()
        .append(true)
        .open(run.join("events.jsonl"))
        .and_then(|mut file| std::io::Write::write_all(&mut file, b"{torn"))
        .expect("append torn line");
    let before = fs::read(run.join("events.jsonl")).expect("events before");
    let report =
        doctor::run(&LocalEffects::new(fixture.layout()).search_path(fixture.bin.as_os_str()));
    let after = fs::read(run.join("events.jsonl")).expect("events after");
    let recovery = diagnostic(&report, Area::RecoveryState);
    assert_eq!((before == after, recovery.status), (true, Status::Warning));
}

#[test]
fn pending_migration_is_a_recovery_error() {
    let fixture = healthy_fixture("doctor-migration");
    fs::write(fixture.layout().root().join("migration.json"), "{}").expect("marker");
    let effects = LocalEffects::new(fixture.layout()).search_path(fixture.bin.as_os_str());
    let status = diagnostic(&doctor::run(&effects), Area::RecoveryState).status;
    assert_eq!(status, Status::Error);
}

#[test]
fn configured_adapter_and_settings_failures_are_diagnostic_errors() {
    let fixture = Fixture::new("doctor-errors");
    let credential = fixture.credential_file();
    fixture.configure(Some("copilot"), CredentialSource::File { path: credential });
    fixture.executable("git");
    fixture.executable("unshare");
    fs::set_permissions(
        fixture.layout().credentials(),
        fs::Permissions::from_mode(0o755),
    )
    .expect("open permissions");
    let effects = LocalEffects::new(fixture.layout()).search_path(fixture.bin.as_os_str());
    let report = doctor::run(&effects);
    let adapter = diagnostic(&report, Area::Adapters).status;
    let local = diagnostic(&report, Area::LocalState).status;
    fs::write(fixture.layout().settings(), "not: [valid").expect("corrupt settings");
    let settings = LocalEffects::new(fixture.layout()).search_path(OsStr::new(""));
    let config = diagnostic(&doctor::run(&settings), Area::ConfigSource).status;
    assert_eq!(
        (adapter, local, config),
        (Status::Error, Status::Warning, Status::Error)
    );
}

#[test]
fn environment_credentials_resolve_from_names_without_values() {
    let fixture = Fixture::new("doctor-environment");
    fixture.configure(
        None,
        CredentialSource::Environment {
            variable: "MODEL_TOKEN".to_owned(),
        },
    );
    for binary in ["git", "unshare"] {
        fixture.executable(binary);
    }
    let _state = fixture.state();
    let found = LocalEffects::new(fixture.layout())
        .search_path(fixture.bin.as_os_str())
        .environment_names([OsString::from("MODEL_TOKEN")]);
    let missing = LocalEffects::new(fixture.layout())
        .search_path(fixture.bin.as_os_str())
        .environment_names([]);
    let found = diagnostic(&doctor::run(&found), Area::CredentialReferences).status;
    let missing = diagnostic(&doctor::run(&missing), Area::CredentialReferences).status;
    assert_eq!((found, missing), (Status::Ok, Status::Error));
}

fn healthy_fixture(label: &str) -> Fixture {
    let fixture = Fixture::new(label);
    let credential = fixture.credential_file();
    fixture.configure(Some("copilot"), CredentialSource::File { path: credential });
    for binary in ["git", "unshare", "copilot"] {
        fixture.executable(binary);
    }
    let _state = fixture.state();
    let _run = fixture.run("finished", true, true);
    fixture
}

/// Declares the reserved config credential with an identity, the way an
/// operator who wants the config clone pinned to one account would.
fn declare_config_identity(fixture: &Fixture, identity: &str) {
    let path = fixture.layout().settings();
    let mut settings = bureau::setup::load_settings(path).expect("settings");
    let source = CredentialSource::Environment {
        variable: "BUREAU_CONFIG_TOKEN".to_owned(),
    };
    settings.credentials.insert(
        "config".to_owned(),
        Credential::new(source).as_identity(identity),
    );
    bureau::setup::save_settings(path, &settings).expect("save settings");
}

/// Points every registered repo at `reference`, on `url`.
fn registry_repo(fixture: &Fixture, reference: &str, url: &str) {
    let path = fixture.layout().config_cache().join("active.json");
    let bytes = fs::read(&path).expect("active config");
    let mut active: bureau::config::ActivatedConfig =
        serde_json::from_slice(&bytes).expect("parse active config");
    for repo in active.config.repos.values_mut() {
        reference.clone_into(&mut repo.credential);
        url.clone_into(&mut repo.url);
    }
    let bytes = serde_json::to_vec_pretty(&active).expect("serialize active config");
    fs::write(&path, bytes).expect("write active config");
}

/// The reference and repo URL of every credential doctor would verify.
fn targets(fixture: &Fixture) -> Vec<(String, String)> {
    LocalEffects::new(fixture.layout())
        .search_path(fixture.bin.as_os_str())
        .identity_targets()
        .into_iter()
        .map(|target| (target.reference, target.repo.url))
        .collect()
}

/// The reserved config credential is verified too: the runner clones the
/// reviewed config remote with it before any registry exists, so a
/// declared identity is checked against that remote and reported beside
/// the registry's own credentials.
#[test]
fn credential_identity_includes_the_declared_config_credential() {
    let fixture = healthy_fixture("doctor-config-identity");
    declare_config_identity(&fixture, "bureau-bot");
    let declared: Vec<Option<String>> = LocalEffects::new(fixture.layout())
        .identity_targets()
        .into_iter()
        .map(|target| target.declared)
        .collect();
    assert_eq!(
        (targets(&fixture), declared),
        (
            vec![
                ("config".to_owned(), CONFIG_REMOTE.to_owned()),
                ("work".to_owned(), REPO_URL.to_owned()),
            ],
            vec![Some("bureau-bot".to_owned()), None]
        )
    );
}

/// A registry repo naming the reserved reference on the config remote's
/// own host is one question, not two: with config committed in a work
/// repo, that repo and the config source are the same destination, and
/// the credential is not offered to it twice.
#[test]
fn a_registry_repo_on_the_config_host_is_not_checked_twice() {
    let fixture = healthy_fixture("doctor-config-shared");
    declare_config_identity(&fixture, "bureau-bot");
    registry_repo(&fixture, "config", CONFIG_REMOTE);
    assert_eq!(
        targets(&fixture),
        vec![("config".to_owned(), CONFIG_REMOTE.to_owned())]
    );
}

/// The same reference on another host is another question, and doctor
/// asks it — exactly as a run does before it spawns.
#[test]
fn the_same_reference_on_another_host_is_checked_there_too() {
    let fixture = healthy_fixture("doctor-config-hosts");
    declare_config_identity(&fixture, "bureau-bot");
    registry_repo(&fixture, "config", "https://other.invalid/work.git");
    assert_eq!(
        targets(&fixture),
        vec![
            ("config".to_owned(), CONFIG_REMOTE.to_owned()),
            (
                "config".to_owned(),
                "https://other.invalid/work.git".to_owned()
            ),
        ]
    );
}

fn diagnostic(report: &doctor::Report, area: Area) -> &doctor::Diagnostic {
    report
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.area == area)
        .expect("diagnostic area")
}
