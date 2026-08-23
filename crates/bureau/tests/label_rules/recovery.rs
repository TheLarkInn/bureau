//! Recovery behavior for interrupted and partial label updates.

use bureau::forge::Forge as _;
use bureau::state::LabelRuleEventKind;

use super::{World, interrupted_audit, item};

fn event_kinds(world: &World) -> Vec<LabelRuleEventKind> {
    world.events().iter().map(|event| event.kind).collect()
}

async fn seed_partial_failure(world: &World, audit: &bureau::state::LabelRuleAudit) {
    world
        .store
        .begin_label_rule_update(audit, 20, "starting")
        .expect("start audit");
    world
        .store
        .record_label_rule_event(audit, LabelRuleEventKind::UpdateFailed, "partial failure")
        .expect("failure audit");
    world
        .forge
        .set_labels(&audit.item, &[])
        .await
        .expect("partial");
}

#[tokio::test]
async fn interrupted_applied_update_gets_a_terminal_audit_event() {
    let world = World::new(&["42"], 20);
    let audit = interrupted_audit("42");
    world
        .store
        .begin_label_rule_update(&audit, 20, "starting")
        .expect("start audit");
    world
        .forge
        .set_labels(&audit.item, &["agent-eligible".to_owned()])
        .await
        .expect("simulate applied update");
    world.pass().await.expect("recovery pass");
    assert_eq!(
        event_kinds(&world),
        [
            LabelRuleEventKind::UpdateStarted,
            LabelRuleEventKind::UpdateApplied,
        ]
    );
}

#[tokio::test]
async fn failed_partial_update_retries_after_leaving_the_filter() {
    let world = World::new(&["42"], 20);
    let audit = interrupted_audit("42");
    seed_partial_failure(&world, &audit).await;
    world.pass().await.expect("retry pass");
    let observed = (world.forge.labels_of(&audit.item), event_kinds(&world));
    assert_eq!(
        observed,
        (
            vec!["agent-eligible".to_owned()],
            vec![
                LabelRuleEventKind::UpdateStarted,
                LabelRuleEventKind::UpdateFailed,
                LabelRuleEventKind::UpdateAbandoned,
                LabelRuleEventKind::UpdateStarted,
                LabelRuleEventKind::UpdateApplied,
            ],
        )
    );
}

#[tokio::test]
async fn changed_config_explicitly_supersedes_the_old_attempt() {
    let world = World::new(&["42"], 20);
    let mut audit = interrupted_audit("42");
    audit.add_labels = vec!["old-eligible".to_owned()];
    audit.remove_labels = vec!["old-blocked".to_owned()];
    world
        .store
        .begin_label_rule_update(&audit, 20, "old rule")
        .expect("old attempt");
    world.pass().await.expect("new rule applies");
    assert_eq!(
        event_kinds(&world),
        [
            LabelRuleEventKind::UpdateStarted,
            LabelRuleEventKind::UpdateAbandoned,
            LabelRuleEventKind::UpdateStarted,
            LabelRuleEventKind::UpdateApplied,
        ]
    );
}

#[tokio::test]
async fn changed_source_abandons_the_old_repository_attempt() {
    let world = World::new(&["42"], 20);
    let mut audit = interrupted_audit("42");
    audit.source = "old/repository".to_owned();
    world
        .store
        .begin_label_rule_update(&audit, 20, "old source")
        .expect("old attempt");
    world.pass().await.expect("new source applies");
    assert_eq!(
        event_kinds(&world),
        [
            LabelRuleEventKind::UpdateStarted,
            LabelRuleEventKind::UpdateAbandoned,
            LabelRuleEventKind::UpdateStarted,
            LabelRuleEventKind::UpdateApplied,
        ]
    );
}

#[tokio::test]
async fn unreadable_recovery_item_does_not_wedge_other_candidates() {
    let world = World::new(&["42", "43"], 20);
    let audit = interrupted_audit("42");
    world
        .store
        .begin_label_rule_update(&audit, 20, "starting")
        .expect("start audit");
    world.forge.remove_item(&audit.item);
    world.pass().await.expect("other item still applies");
    assert_eq!(
        world.forge.labels_of(&item("43").external_id),
        vec!["agent-eligible".to_owned()]
    );
}
