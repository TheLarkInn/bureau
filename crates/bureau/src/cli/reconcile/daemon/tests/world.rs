use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bureau::contract::Trust;
use bureau::forge::fake::FakeForge;
use bureau::forge::{Dependency, Forge, Item, LabelForge};
use bureau::process::Secret;
use bureau::state::{LabelRuleEventKind, TerminalRecord};

use super::super::{Daemon, RevisionBuilder, new, revision};
use super::files::Files;

fn item(id: &str) -> Item {
    Item {
        external_id: format!("offline/work#{id}"),
        title: format!("Item {id}"),
        body: String::new(),
        url: format!("fake://item/{id}"),
        labels: vec!["blocked".into()],
        trust: Trust::Untrusted,
    }
}

fn labels() -> Arc<FakeForge> {
    let forge = Arc::new(FakeForge::new(vec![item("3")]));
    forge.set_dependencies(
        &item("3").external_id,
        vec![Dependency {
            external_id: "offline/work#4".into(),
            closed: true,
        }],
    );
    forge
}

fn builder(
    labels: Arc<FakeForge>,
    errors: Arc<Mutex<BTreeMap<String, String>>>,
    repository_collision: bool,
) -> Box<RevisionBuilder> {
    let factory = Arc::new(FakeForge::new(vec![item("1")]));
    let ordinary = Arc::new(FakeForge::new(vec![item("2")]));
    Box::new(move |active, settings| {
        let mut current = revision(active, settings)?;
        errors
            .lock()
            .expect("observed errors")
            .clone_from(&current.credentials.errors);
        if repository_collision {
            current
                .credentials
                .values
                .insert("missing-model".into(), Secret::new("repo-value"));
        }
        current.forges = BTreeMap::from([
            ("factory".into(), factory.clone() as Arc<dyn Forge>),
            ("ordinary".into(), ordinary.clone() as Arc<dyn Forge>),
        ]);
        current.label_forges =
            BTreeMap::from([("graduate".into(), labels.clone() as Arc<dyn LabelForge>)]);
        Ok(current)
    })
}

pub(super) struct World {
    pub(super) daemon: Daemon,
    labels: Arc<FakeForge>,
    errors: Arc<Mutex<BTreeMap<String, String>>>,
    files: Files,
}

impl World {
    pub(super) fn root(&self) -> &std::path::Path {
        &self.files.root
    }

    pub(super) fn new(repository_collision: bool) -> Self {
        let files = Files::new();
        let mut daemon = new(&files.args()).expect("daemon setup");
        let labels = labels();
        let errors = Arc::new(Mutex::new(BTreeMap::new()));
        daemon.revision = builder(labels.clone(), errors.clone(), repository_collision);
        daemon.recovery_forge = Box::new(|_, _, _| Ok(Arc::new(FakeForge::default())));
        Self {
            daemon,
            labels,
            errors,
            files,
        }
    }

    pub(super) async fn finish(&mut self) {
        tokio::time::timeout(Duration::from_secs(30), async {
            while !self.daemon.active_ids().is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
                self.daemon.active.reap().await;
            }
        })
        .await
        .expect("ordinary run finishes");
    }

    pub(super) fn records(&self) -> Vec<TerminalRecord> {
        self.daemon.engine.finished().expect("finished run records")
    }

    pub(super) fn factory_leases(&self) -> usize {
        self.daemon
            .state
            .active("factory")
            .expect("factory claims")
            .len()
    }

    pub(super) fn factory_headroom(&self) -> usize {
        let limits = bureau::config::Limits {
            max_concurrent: Some(1),
            max_runs_per_hour: Some(1),
            ..Default::default()
        };
        self.daemon
            .state
            .headroom("factory", &limits, 0)
            .expect("factory budget")
    }

    pub(super) fn labels(&self) -> Vec<String> {
        self.labels.labels_of(&item("3").external_id)
    }

    pub(super) fn label_events(&self) -> Vec<LabelRuleEventKind> {
        self.daemon
            .state
            .label_rule_events("graduate")
            .expect("label audit")
            .into_iter()
            .map(|event| event.kind)
            .collect()
    }

    pub(super) fn errors(&self) -> BTreeMap<String, String> {
        self.errors.lock().expect("retained model errors").clone()
    }
}
