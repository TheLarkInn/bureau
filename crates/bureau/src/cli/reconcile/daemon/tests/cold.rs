mod continuation;
mod evidence;
mod fixture;
mod ordinary;

use bureau::state::LabelRuleEventKind;

use super::super::deferred::Deferred;
use super::world::World;
use fixture::{Paused, remove_config};

async fn idle_failure(world: &mut World, paused: &Paused, reference: &str) {
    let error = world
        .daemon
        .pass()
        .await
        .expect_err("idle deferred recovery must fail visibly");
    assert_eq!(
        (
            error.downcast_ref::<Deferred>().is_some(),
            error.to_string().contains(&paused.snapshot.run_id),
            error.to_string().contains(reference),
            world.daemon.active_ids().len()
        ),
        (true, true, true, 0),
    );
    paused.assert_preserved(world);
}

async fn independent_work(world: &mut World, paused: &Paused) {
    world
        .daemon
        .pass()
        .await
        .expect("cold model deferral must not abort independent work");
    super::assert_only_ordinary_started(world);
    world.finish().await;
    super::assert_independent_progress(world);
    paused.assert_preserved(world);
}

#[tokio::test]
async fn paused_factory_with_unavailable_saved_source_preserves_evidence_and_allows_progress() {
    let mut world = World::new(false);
    let paused = Paused::create(&mut world).await;
    independent_work(&mut world, &paused).await;
    idle_failure(&mut world, &paused, "copilot-model").await;
    super::assert_independent_progress(&world);
}

#[tokio::test]
async fn label_only_progress_is_not_mistaken_for_an_idle_deferred_pass() {
    let mut world = World::new(false);
    let paused = Paused::create(&mut world).await;
    remove_config(&world, &["assignments/ordinary.yaml"]);
    world
        .daemon
        .pass()
        .await
        .expect("independent label action is actual progress");
    assert_eq!(
        (
            world.daemon.active_ids(),
            world.labels(),
            world.label_events()
        ),
        (
            vec![],
            vec!["eligible".into()],
            vec![
                LabelRuleEventKind::UpdateStarted,
                LabelRuleEventKind::UpdateApplied
            ]
        ),
    );
    idle_failure(&mut world, &paused, "copilot-model").await;
}

fn assert_recovered(world: &World, active: Vec<String>, run_id: String) {
    let finished: Vec<_> = world
        .records()
        .into_iter()
        .map(|record| record.snapshot.run_id)
        .collect();
    assert_eq!((active, finished), (vec![run_id.clone()], vec![run_id]));
}

async fn recovery_progress(world: &mut World, paused: &Paused) {
    let run_id = ordinary::paused(world).await;
    remove_config(
        world,
        &[
            "assignments/ordinary.yaml",
            "assignments/factory.yaml",
            "label_rules/graduate.yaml",
        ],
    );
    world
        .daemon
        .pass()
        .await
        .expect("later independent cold recovery must proceed");
    let active = world.daemon.active_ids();
    world.finish().await;
    assert_recovered(world, active, run_id);
    paused.assert_preserved(world);
}

#[tokio::test]
async fn independent_recovery_continues_after_a_deferred_saved_factory() {
    let mut world = World::new(false);
    let paused = Paused::create(&mut world).await;
    recovery_progress(&mut world, &paused).await;
    idle_failure(&mut world, &paused, "copilot-model").await;
}

#[tokio::test]
async fn a_lost_ownership_fence_is_never_a_credential_deferral() {
    let mut world = World::new(false);
    let paused = Paused::create(&mut world).await;
    let owner = world
        .daemon
        .owner(&paused.snapshot)
        .expect("replacement owner");
    assert!(
        owner
            .claim(bureau::supervise::LEASE_TTL)
            .expect("recovery claim")
    );
    owner
        .release()
        .expect("lose ownership before handling missing model source");
    let error = world
        .daemon
        .resume_owned(paused.snapshot.clone(), owner)
        .err()
        .expect("stale fence must fail");
    assert!(matches!(
        error.downcast_ref::<bureau::state::Error>(),
        Some(bureau::state::Error::LeaseLost(_))
    ));
    paused.assert_preserved(&world);
}

fn change_header(paused: &Paused) {
    let mut events = bureau::runlog::read_events(&paused.directory).expect("original real log");
    events[0].data["run_id"] = serde_json::json!("different-bureau-run");
    let lines: Vec<_> = events
        .iter()
        .map(|event| serde_json::to_string(event).expect("event"))
        .collect();
    std::fs::write(
        paused.directory.join(bureau::runlog::EVENTS_FILE),
        lines.join("\n") + "\n",
    )
    .expect("deliberate negative identity corruption");
}

async fn fatal_pass(world: &mut World, paused: &Paused) {
    let before = evidence::unchanged_tree(&paused.directory);
    let error = world
        .daemon
        .pass()
        .await
        .expect_err("corruption is fatal, not unavailable auth");
    assert_eq!(
        (
            error.downcast_ref::<Deferred>().is_none(),
            world.daemon.active_ids(),
            world.labels(),
            evidence::unchanged_tree(&paused.directory)
        ),
        (true, vec![], vec!["blocked".into()], before),
    );
}

#[tokio::test]
async fn corrupt_saved_identity_is_not_hidden_by_an_unavailable_model_source() {
    let mut world = World::new(false);
    let paused = Paused::create(&mut world).await;
    change_header(&paused);
    fatal_pass(&mut world, &paused).await;
}

#[tokio::test]
async fn corrupt_factory_log_is_not_hidden_by_an_unavailable_model_source() {
    let mut world = World::new(false);
    let paused = Paused::create(&mut world).await;
    let path = paused.directory.join(bureau::runlog::EVENTS_FILE);
    let text = std::fs::read_to_string(&path).expect("real paused log");
    let mut lines: Vec<_> = text.lines().collect();
    lines[1] = "invalid interior event";
    std::fs::write(path, lines.join("\n") + "\n").expect("deliberate interior log corruption");
    fatal_pass(&mut world, &paused).await;
}

#[tokio::test]
async fn a_torn_tail_is_not_repaired_while_factory_credentials_are_deferred() {
    use std::io::Write as _;
    let mut world = World::new(false);
    let paused = Paused::create(&mut world).await;
    let mut log = std::fs::OpenOptions::new()
        .append(true)
        .open(paused.directory.join(bureau::runlog::EVENTS_FILE))
        .expect("real log");
    log.write_all(br#"{"seq":"#)
        .expect("simulate an interrupted future append");
    let before = evidence::Evidence::capture(&paused.directory);
    world
        .daemon
        .pass()
        .await
        .expect("defer without truncating authoritative bytes");
    world.finish().await;
    assert_eq!(evidence::Evidence::capture(&paused.directory), before);
}

#[tokio::test]
async fn replaced_workspace_identity_is_not_a_credential_deferral() {
    let mut world = World::new(false);
    let paused = Paused::create(&mut world).await;
    let workspace = paused.directory.join("wt");
    std::fs::rename(&workspace, paused.directory.join("original-wt"))
        .expect("retain original tree");
    std::fs::create_dir(&workspace).expect("replace path with a different inode");
    fatal_pass(&mut world, &paused).await;
}
