use std::path::Path;

use bureau::runlog::copilot_factory::{Record, Records};

fn digest(path: &Path) -> String {
    bureau_plugin::tree_digest(path).expect("unchanged native fixture tree")
}

fn record(directory: &Path) -> Record {
    Records::replay(&bureau::runlog::read_events_tolerant(directory).expect("real factory events"))
        .expect("strict native replay")
        .0
        .into_values()
        .next()
        .expect("known factory")
}

#[derive(Debug, PartialEq)]
pub(super) struct Evidence {
    pub(super) record: Record,
    run: String,
    session: String,
    workspace: String,
    trace: Vec<u8>,
}

impl Evidence {
    pub(super) fn capture(directory: &Path) -> Self {
        let record = record(directory);
        Self {
            run: digest(directory),
            session: digest(&record.intent.paths.storage.session),
            workspace: digest(&record.intent.workspace.directory),
            trace: std::fs::read(record.intent.paths.storage.copilot_home.join("trace.jsonl"))
                .expect("original offline RPC trace"),
            record,
        }
    }

    pub(super) fn assert_continuation(&self, expected: (bool, bool, usize)) {
        let calls: Vec<serde_json::Value> = std::str::from_utf8(&self.trace)
            .expect("trace UTF-8")
            .lines()
            .map(|line| serde_json::from_str(line).expect("trace entry"))
            .collect();
        let count = |method: &str| calls.iter().filter(|call| call["method"] == method).count();
        assert_eq!(
            (
                self.record.can_start(),
                self.record.can_resume(),
                self.record.execution_clean,
                count("session.factory.run"),
                count("session.factory.resume")
            ),
            (expected.0, expected.1, true, expected.2, 0),
        );
    }
}

pub(super) fn unchanged_tree(directory: &Path) -> String {
    digest(directory)
}
