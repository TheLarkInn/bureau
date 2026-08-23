//! Loading coverage for the documented label-rule YAML surface.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bureau::config::{Config, LabelRuleCondition};

static NEXT: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "bureau-label-rule-config-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, relative: &str, text: &str) {
    let path = path.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    std::fs::write(path, text).expect("write fixture");
}

#[test]
fn agreed_label_rule_yaml_loads() {
    let dir = TestDir::new();
    write(
        &dir.0,
        "repos.yaml",
        "repos:\n  bureau:\n    url: https://github.com/TheLarkInn/bureau\n    forge: github\n    access: read\n    credential: github\n",
    );
    write(
        &dir.0,
        "label_rules/graduate-unblocked.yaml",
        "name: graduate-unblocked\nwork:\n  forge: github\n  source: TheLarkInn/bureau\n  filter: \"is:issue is:open label:agent-blocked\"\nwhen: dependencies_closed\nadd_labels: [agent-eligible]\nremove_labels: [agent-blocked]\nlimits:\n  max_updates_per_hour: 20\n",
    );
    let config = Config::load(&dir.0).expect("label rule config");
    let loaded = &config.label_rules["graduate-unblocked"];
    assert_eq!(
        (
            loaded.when,
            loaded.add_labels.first().map(String::as_str),
            loaded.limits.max_updates_per_hour,
        ),
        (
            LabelRuleCondition::DependenciesClosed,
            Some("agent-eligible"),
            20,
        )
    );
}

#[test]
fn source_must_match_the_registered_github_host() {
    let dir = TestDir::new();
    write(
        &dir.0,
        "repos.yaml",
        "repos:\n  bureau:\n    url: https://ghe.example/TheLarkInn/bureau\n    forge: github\n    access: read\n    credential: github\n",
    );
    write(
        &dir.0,
        "label_rules/graduate-unblocked.yaml",
        "name: graduate-unblocked\nwork:\n  forge: github\n  source: TheLarkInn/bureau\n  filter: is:issue\nwhen: dependencies_closed\nadd_labels: [agent-eligible]\nremove_labels: [agent-blocked]\nlimits:\n  max_updates_per_hour: 20\n",
    );
    let errors = Config::load(&dir.0).expect_err("host mismatch");
    assert!(errors[0].message.contains("must match a registered repo"));
}
