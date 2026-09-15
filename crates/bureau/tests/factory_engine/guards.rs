use std::os::unix::fs::PermissionsExt as _;

use bureau::runlog::EventKind;

use super::fixture::{Fixture, write};

#[tokio::test]
async fn incomplete_or_invalid_tool_initialization_never_admits_a_factory() {
    for mode in ["invalid-initialize-ack", "null-tool-metadata"] {
        let fixture = Fixture::create(mode);
        let outcome = fixture.engine.run(&fixture.plan).await;
        assert_eq!(
            (
                fixture.calls("session.factory.run"),
                fixture.count(EventKind::StepFinished),
                fixture.calls("session.tools.initializeAndValidate")
            ),
            (0, 0, 1),
            "{mode}: {outcome:?}"
        );
    }
}

#[tokio::test]
async fn pending_broker_connections_and_display_labels_preserve_exact_namespace_grants() {
    for (mode, polls) in [("mcp-pending", 2), ("mcp-display-label", 1)] {
        let fixture = Fixture::create(mode);
        let outcome = fixture.engine.run(&fixture.plan).await;
        assert_eq!(
            (
                fixture.calls("session.factory.run"),
                fixture.record().can_clean(),
                fixture.calls("session.mcp.list")
            ),
            (1, true, polls),
            "{mode}: {outcome:?}"
        );
    }
}

#[tokio::test]
async fn an_unconfigured_broker_is_never_treated_as_ready() {
    let fixture = Fixture::create("mcp-not-configured");
    let _outcome = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.calls("session.factory.run"),
            fixture.count(EventKind::StepFinished)
        ),
        (0, 0)
    );
}

#[tokio::test]
async fn invalid_runtime_identity_catalog_permissions_or_accounting_never_finish() {
    for mode in [
        "wrong-session",
        "unapproved-tool",
        "wrong-run",
        "missing-accounting",
        "bad-permission-ack",
        "accounting-incomplete",
        "terminal-status-conflict",
        "unrequested-cancel",
    ] {
        let fixture = Fixture::create(mode);
        let outcome = fixture.engine.run(&fixture.plan).await;
        assert_eq!(
            (
                fixture.count(EventKind::StepFinished),
                fixture.count(EventKind::RunFinished),
                fixture.directory().join("PAUSE").exists(),
                fixture.record().can_resume()
            ),
            (0, 0, true, false),
            "{mode}: {outcome:?}"
        );
    }
}

#[tokio::test]
async fn accounting_failure_preserves_native_counters_without_claiming_complete_cost() {
    let fixture = Fixture::create("accounting-incomplete");
    let outcome = fixture.engine.run(&fixture.plan).await;
    let record = fixture.record();
    assert_eq!(
        (
            record.accounting_complete(),
            record.consumed.map(|usage| usage.nano_aiu),
            outcome.cost_usd,
            fixture.count(EventKind::StepFinished)
        ),
        (false, Some(1_000_000_000), 0.01, 0)
    );
}

#[tokio::test]
async fn missing_runtime_database_is_not_recreated_on_reentry() {
    let fixture = Fixture::create("pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    let database = fixture
        .record()
        .intent
        .paths
        .storage
        .session
        .join("session.db");
    std::fs::remove_file(&database).expect("remove saved database");
    std::fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit resume");
    let _second = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            database.exists(),
            fixture.calls("session.resume"),
            fixture.calls("session.factory.resume"),
            fixture.count(EventKind::RunFinished)
        ),
        (false, 0, 0, 0)
    );
}

#[tokio::test]
async fn changed_provider_pin_is_not_replaced_from_approved_source() {
    let fixture = Fixture::create("pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    let pin = fixture.record().intent.paths.provider.join("extension.mjs");
    std::fs::set_permissions(&pin, std::fs::Permissions::from_mode(0o644))
        .expect("owned pin permissions");
    write(&pin, "changed approved pin");
    std::fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit resume");
    let _second = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.calls("session.resume"),
            std::fs::read_to_string(pin).expect("changed bytes"),
            fixture.count(EventKind::StepFinished)
        ),
        (0, "changed approved pin".into(), 0)
    );
}

#[tokio::test]
async fn original_sources_are_not_required_for_native_resume() {
    let fixture = Fixture::create("pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    std::fs::remove_dir_all(fixture.root.join("runtime-source")).expect("remove original runtime");
    std::fs::remove_dir_all(fixture.directory().join("wt/.github/extensions"))
        .expect("remove project source");
    std::fs::remove_file(fixture.directory().join("PAUSE")).expect("explicit resume");
    let _second = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.record().attempt.map(std::num::NonZeroU64::get),
            fixture.calls("session.factory.run"),
            fixture.calls("session.factory.resume")
        ),
        (Some(2), 1, 1)
    );
}

#[tokio::test]
async fn a_recreated_worktree_path_cannot_replace_native_factory_effects() {
    let fixture = Fixture::create("pause");
    let _first = fixture.engine.run(&fixture.plan).await;
    let directory = fixture.directory();
    std::fs::rename(directory.join("wt"), directory.join("original-wt")).expect("retain original");
    std::fs::create_dir(directory.join("wt")).expect("replacement directory");
    std::fs::remove_file(directory.join("PAUSE")).expect("explicit resume");
    let _second = fixture.engine.run(&fixture.plan).await;
    assert_eq!(
        (
            fixture.calls("session.factory.resume"),
            fixture.count(EventKind::RunFinished),
            directory
                .join("original-wt/factory-uncommitted.txt")
                .exists()
        ),
        (0, 0, true)
    );
}
