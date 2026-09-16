use std::collections::BTreeMap;
use std::path::PathBuf;

use bureau::engine::RunPlan;
use bureau::runlog::RunSnapshot;
use bureau::setup::CredentialSource;

use super::super::world::World;
use super::super::{native_plan, native_source};
use super::evidence::Evidence;

fn declared_sources(world: &mut World) -> PathBuf {
    let source = world.root().join("saved-model-credential");
    native_source::write(&source, "offline-model-token");
    let repo = world.root().join("repo-credential");
    let settings = world.daemon.settings.as_mut().expect("fixture settings");
    settings.credentials.insert(
        "copilot-model".into(),
        CredentialSource::File {
            path: source.clone(),
        },
    );
    settings
        .credentials
        .insert("unused".into(), CredentialSource::File { path: repo });
    source
}

fn plan(world: &World, mode: &str) -> RunPlan {
    native_source::bureau_executable(world.root());
    let repository = native_source::repository(world.root());
    let runtime = native_source::runtime(world.root(), mode);
    native_plan::build(&repository, &runtime, mode)
}

pub(super) fn prepare(world: &mut World, mode: &str) -> (RunPlan, PathBuf) {
    let source = declared_sources(world);
    (plan(world, mode), source)
}

pub(super) struct Paused {
    pub(super) snapshot: RunSnapshot,
    pub(super) directory: PathBuf,
    original: Evidence,
}

impl Paused {
    pub(super) async fn execute(world: &World, mut plan: RunPlan, source: PathBuf) -> Self {
        let owner = world
            .daemon
            .owner(&plan.snapshot())
            .expect("original native owner");
        assert!(
            owner
                .claim(bureau::supervise::LEASE_TTL)
                .expect("initial factory claim")
        );
        plan.lease = Some(owner.clone());
        let _outcome = world.daemon.engine.run(&plan).await;
        owner.release().expect("release paused original owner");
        let directory = world.daemon.engine.runs_dir.join(&plan.run_id);
        let original = Evidence::capture(&directory);
        std::fs::remove_file(source).expect("declared model source becomes unavailable");
        Self {
            snapshot: plan.snapshot(),
            directory,
            original,
        }
    }

    pub(super) fn assert_continuation(&self, expected: (bool, bool, usize)) {
        self.original.assert_continuation(expected);
    }

    pub(super) async fn create(world: &mut World) -> Self {
        let (plan, source) = prepare(world, "pause");
        let paused = Self::execute(world, plan, source).await;
        paused.assert_continuation((false, true, 1));
        paused
    }

    pub(super) fn assert_preserved(&self, world: &World) {
        assert_eq!(Evidence::capture(&self.directory), self.original);
        let work = world
            .daemon
            .state
            .preserved_factory_work(
                &world.daemon.engine.runs_dir,
                &self.snapshot.assignment.name,
                "github",
            )
            .expect("authoritative saved factory reservation");
        let leases = world
            .daemon
            .state
            .active(&self.snapshot.assignment.name)
            .expect("factory leases");
        assert_eq!(
            (work, leases.len()),
            (
                BTreeMap::from([(
                    self.snapshot.item.external_id.clone(),
                    self.snapshot.run_id.clone()
                )]),
                0
            ),
        );
    }
}

pub(super) fn remove_config(world: &World, files: &[&str]) {
    let source = world.root().join("source");
    for file in files {
        std::fs::remove_file(source.join(".bureau").join(file)).expect("remove fixture policy");
    }
    native_source::commit(&source);
}
