pub use bureau_lifecycle::setup;

use std::path::PathBuf;

#[path = "init_flow/support.rs"]
mod support;

use setup::{FirstPipeline, FlowError, InitFlow, InitState};
use support::{
    FakeEffects, config_draft, expected_authored_events, expected_events, has_run_effects, request,
    source,
};

#[test]
fn fixed_single_repository_flow_is_strictly_ordered() {
    let selection = FirstPipeline::Fixed;
    let draft = config_draft(&selection);
    let request = request(source(true), selection, true);
    let mut flow = InitFlow::new(request);
    let mut effects = FakeEffects::new(false);
    let outcome = flow.run(&mut effects).expect("complete init");
    let actual = (
        effects.events,
        effects.proposed,
        flow.state(),
        outcome.config.commit,
        outcome.outcomes.runs.len(),
    );
    let expected = (
        expected_events().map(str::to_owned).to_vec(),
        Some(draft),
        InitState::Complete,
        "merge-7".to_owned(),
        1,
    );
    assert_eq!(actual, expected);
}

#[test]
fn ai_authored_separate_repository_uses_injected_author() {
    let first = FirstPipeline::AiAuthored {
        request: "author the first pipeline".to_owned(),
    };
    let mut flow = InitFlow::new(request(source(false), first, false));
    let mut effects = FakeEffects::new(false);
    flow.run(&mut effects).expect("complete authored init");
    let actual = (effects.events, effects.proposed);
    let expected = (
        expected_authored_events().map(str::to_owned).to_vec(),
        Some(config_draft(&FirstPipeline::AiAuthored {
            request: "author the first pipeline".to_owned(),
        })),
    );
    assert_eq!(actual, expected);
}

#[test]
fn explicit_migration_precedes_config_preparation() {
    let mut request = request(source(true), FirstPipeline::Fixed, false);
    request.settings.migration.source = Some(PathBuf::from("/old/bureau"));
    let mut flow = InitFlow::new(request);
    let mut effects = FakeEffects::new(false);
    flow.run(&mut effects).expect("complete migrated init");
    let migration = effects
        .events
        .iter()
        .position(|event| event == "migrate_local_state");
    let preparation = effects
        .events
        .iter()
        .position(|event| event == "prepare_config:fixed");
    assert!(migration.is_some() && migration < preparation);
}

#[test]
fn init_rejects_existing_settings_before_other_effects() {
    let first = FirstPipeline::Fixed;
    let mut flow = InitFlow::new(request(source(true), first, false));
    let mut effects = FakeEffects::new(true);
    let error = flow.run(&mut effects).expect_err("init must be first-time");
    let actual = (
        matches!(error, FlowError::SettingsAlreadyExist),
        effects.events,
        flow.state(),
    );
    assert_eq!(
        actual,
        (
            true,
            vec!["settings_exist".to_owned()],
            InitState::CheckingSettings
        )
    );
}

#[test]
fn unmerged_config_is_never_written_or_reconciled() {
    let first = FirstPipeline::Fixed;
    let mut flow = InitFlow::new(request(source(true), first, true));
    let mut effects = FakeEffects::new(false);
    effects.fail_at = Some("wait_for_merge");
    let error = flow.run(&mut effects).expect_err("merge wait fails");
    let actual = (
        matches!(error, FlowError::Effect(_)),
        has_run_effects(&effects.events),
        flow.state(),
    );
    assert_eq!(actual, (true, false, InitState::WaitingForMerge));
}

#[test]
fn merged_commit_mismatch_fails_before_local_or_run_effects() {
    let first = FirstPipeline::Fixed;
    let mut flow = InitFlow::new(request(source(false), first, true));
    let mut effects = FakeEffects::new(false);
    effects.validated_commit = "different-commit".to_owned();
    let error = flow.run(&mut effects).expect_err("commit mismatch");
    let actual = (
        matches!(error, FlowError::MergedCommitMismatch { .. }),
        has_run_effects(&effects.events),
        flow.state(),
    );
    assert_eq!(actual, (true, false, InitState::ValidatingMergedCommit));
}
