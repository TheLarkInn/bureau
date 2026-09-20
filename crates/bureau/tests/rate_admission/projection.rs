use std::collections::BTreeMap;

use bureau::config::{Config, Limits};
use bureau::contract::{StepOutcome, Trust};
use bureau::forge::Item;
use bureau::runlog::{RunFinishedData, RunSnapshot};
use bureau::state::{TerminalRecord, project_terminal};

use super::support::{ASSIGNMENT, Fixture};

const CONFIG: &str = "
repos:
  local: {url: unused, forge: github, access: read, credential: unused}
roles: {}
assignments:
  rate-limited:
    name: rate-limited
    work:
      forge: github
      source: unused
      filter: '*'
      abort_label: 'bureau:failed'
      escalate_label: 'bureau:needs-human'
    repos: [local]
    pipeline: check
    role: unused
    verify: 'true'
    branch_prefix: bureau/
pipelines:
  check:
    name: check
    steps: [{name: check, type: deterministic, run: 'true', next: done}]
";

fn item() -> Item {
    Item {
        external_id: "first".to_owned(),
        title: "offline".to_owned(),
        body: String::new(),
        url: "fake://first".to_owned(),
        labels: Vec::new(),
        trust: Trust::Untrusted,
    }
}

fn snapshot() -> RunSnapshot {
    let mut config: Config = serde_yaml_ng::from_str(CONFIG).expect("snapshot config");
    RunSnapshot {
        run_id: "first-run".to_owned(),
        assignment: config.assignments.remove(ASSIGNMENT).expect("assignment"),
        pipeline: config.pipelines.remove("check").expect("pipeline"),
        repos: config.repos,
        roles: config.roles,
        item: item(),
        config_source: None,
        plugin_sources: BTreeMap::new(),
        direct_agents: BTreeMap::new(),
    }
}

fn terminal() -> TerminalRecord {
    TerminalRecord {
        snapshot: snapshot(),
        finished: RunFinishedData {
            terminal: None,
            outcome: StepOutcome::NoWork,
            message: "done".to_owned(),
            cost_usd: 3.0,
            pr: None,
            disposition: None,
        },
    }
}

fn replay_after_release_failure(fixture: &Fixture, record: &TerminalRecord) -> bool {
    fixture.execute(
        "CREATE TRIGGER fail_release BEFORE DELETE ON leases
         BEGIN SELECT RAISE(ABORT, 'injected release failure'); END;",
    );
    let failed = project_terminal(&fixture.stores[0], record).is_err();
    fixture.execute("DROP TRIGGER fail_release");
    project_terminal(&fixture.stores[0], record).expect("recover projection");
    project_terminal(&fixture.stores[1], record).expect("repeat projection");
    failed
}

#[test]
fn partial_terminal_projection_does_not_double_charge_or_retime_old_admissions() {
    let fixture = Fixture::new("terminal");
    let owner = fixture.owner(0, "first", "first-run");
    let claimed = fixture.claim(&owner, &Limits::default());
    fixture.execute("UPDATE run_admissions SET admitted_at_ms = 7");
    let failed = replay_after_release_failure(&fixture, &terminal());
    let budget = fixture.stores[0].budget(ASSIGNMENT).expect("budget");
    assert_eq!(
        (
            claimed.is_some(),
            failed,
            budget.live_leases,
            budget.runs_this_hour,
            budget.runs_today,
            budget.spent_today_usd,
            fixture.admission_time()
        ),
        (true, true, 0, 0, 0, 3.0, 7)
    );
}
