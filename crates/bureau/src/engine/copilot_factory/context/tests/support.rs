use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bureau_plugin::Resolver;
use serde_json::{Value, json};

use super::super::{PinnedContext, prepared};
use crate::config::Role;
use crate::engine::{RunPlan, rehydrate};
use crate::forge::fake::FakeForge;

static NEXT: AtomicU32 = AtomicU32::new(0);

pub(super) fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture directory");
    fs::write(path, bytes).expect("fixture file");
}

pub(super) fn role(agent: &str) -> Role {
    serde_json::from_value(json!({
        "name": "worker", "agent": agent, "adapter": "copilot",
        "permissions": ["repo:read"], "min_trust": "maintainer"
    }))
    .expect("role")
}

pub(super) fn plan(role: &Role) -> RunPlan {
    let snapshot = serde_json::from_value(json!({
        "run_id": "context-test", "assignment": {
            "name": "review", "work": {"forge": "github", "source": "owner/repo", "filter": ""},
            "repos": [], "pipeline": "review", "role": "worker", "verify": "",
            "branch_prefix": "runner/"
        },
        "pipeline": {"name": "review", "steps": []},
        "roles": {(role.name.clone()): role}, "repos": {},
        "item": {"external_id": "1", "title": "", "body": "", "url": "", "labels": [], "trust": "maintainer"}
    })).expect("snapshot");
    let mut plan = rehydrate(snapshot, Arc::new(FakeForge::default()), BTreeMap::new());
    plan.direct_agents.insert(
        role.name.clone(),
        b"---\nname: worker\n---\nOriginal prompt.\n".to_vec(),
    );
    plan
}

pub(super) fn settings(enabled: Value) -> Value {
    let mut value = json!({
        "extraKnownMarketplaces": {"local": {"source": {"source": "directory", "path": "marketplace"}}}
    });
    value["enabledPlugins"] = enabled;
    value
}

pub(super) struct Fixture {
    root: PathBuf,
    pub worktree: PathBuf,
    pub run: PathBuf,
    pub private: PathBuf,
    pub home: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let parent = std::env::current_dir()
            .expect("test directory")
            .join("target");
        fs::create_dir_all(&parent).expect("fixture parent");
        let root = parent.join(format!(
            "factory-context-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("exclusive fixture root");
        for name in ["worktree", "run", "home"] {
            fs::create_dir(root.join(name)).expect("fixture directory");
        }
        Self {
            worktree: root.join("worktree"),
            private: root.join("run/private"),
            run: root.join("run"),
            home: root.join("home"),
            root,
        }
    }

    pub fn resolver(&self) -> Resolver {
        Resolver::new(&self.run, Some(self.home.clone()))
    }

    pub fn settings_path(&self) -> PathBuf {
        self.worktree.join(".github/copilot/settings.json")
    }

    pub fn set_settings(&self, value: &Value) {
        write(
            &self.settings_path(),
            serde_json::to_vec(value).expect("settings JSON"),
        );
    }

    pub fn plugin(&self, name: &str) -> PathBuf {
        let directory = self.worktree.join("marketplace/plugins").join(name);
        write(
            &directory.join("plugin.json"),
            format!(r#"{{"name":"{name}","version":"1.0.0"}}"#),
        );
        write(
            &directory.join("agents/worker.agent.md"),
            b"---\nname: worker\ntools: [view]\n---\nPinned agent.\n",
        );
        write(
            &directory.join("skills/review/SKILL.md"),
            b"---\nname: review\ndescription: Review\n---\nPinned skill.\n",
        );
        write(&directory.join("scripts/check.sh"), b"#!/bin/sh\nexit 0\n");
        self.catalog(&json!([{"name": name, "source": format!("plugins/{name}")}]));
        directory
    }

    pub fn catalog(&self, entries: &Value) {
        write(
            &self.worktree.join("marketplace/marketplace.json"),
            serde_json::to_vec(&json!({"name": "local", "plugins": entries}))
                .expect("catalog JSON"),
        );
    }

    pub fn pinned_role(&self, plan: &mut RunPlan, role: &Role) -> bureau_plugin::PluginSource {
        let source = self
            .resolver()
            .snapshot_only(&role.agent, &self.worktree)
            .expect("role pin")
            .source;
        plan.plugin_sources
            .insert(source.name.clone(), source.clone());
        source
    }

    pub fn prepare(&self, plan: &RunPlan, role: &Role) -> Result<PinnedContext, String> {
        prepared(plan, role, &self.worktree, &self.private, &self.resolver())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.root);
    }
}
