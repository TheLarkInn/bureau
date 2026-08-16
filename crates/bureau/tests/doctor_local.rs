//! Offline production-effect tests for local doctor diagnostics.

pub mod local_effects_support;

use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::fs::PermissionsExt as _;

use bureau::doctor::{self, Area, LocalEffects, Status};
use bureau::setup::CredentialSource;

use local_effects_support::{Fixture, SECRET_TEXT};

#[test]
fn local_doctor_inspects_all_areas_without_exposing_credentials() {
    let fixture = healthy_fixture("doctor-healthy");
    let effects = LocalEffects::new(fixture.layout()).search_path(fixture.bin.as_os_str());
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

fn diagnostic(report: &doctor::Report, area: Area) -> &doctor::Diagnostic {
    report
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.area == area)
        .expect("diagnostic area")
}
