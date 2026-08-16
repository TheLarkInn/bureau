//! Offline transcript tests for read-only doctor diagnostics.

use std::cell::RefCell;

use bureau_lifecycle::doctor::{self, Area, Observation, Status};

struct DoctorEffects {
    seen: RefCell<Vec<Area>>,
}

impl DoctorEffects {
    const fn new() -> Self {
        Self {
            seen: RefCell::new(Vec::new()),
        }
    }
}

impl doctor::Effects for DoctorEffects {
    fn inspect(&self, area: Area) -> Result<Observation, String> {
        self.seen.borrow_mut().push(area);
        match area {
            Area::CredentialReferences => Ok(Observation::new(
                Status::Warning,
                "reference_missing",
                "credential reference `work` is unresolved",
            )),
            Area::RecoveryState => Err("state database is unreadable".to_owned()),
            _ => Ok(Observation::new(Status::Ok, "checked", "available")),
        }
    }
}

#[test]
fn doctor_checks_every_area_and_renders_human_and_json() {
    let effects = DoctorEffects::new();
    let report = doctor::run(&effects);
    let json: serde_json::Value =
        serde_json::from_str(&report.json().expect("render json")).expect("parse json");
    let actual = (
        effects.seen.into_inner(),
        report.status(),
        report.human().contains("[warning] credential references"),
        json["diagnostics"].as_array().map(Vec::len),
    );
    assert_eq!(
        actual,
        (
            Area::ALL.to_vec(),
            Status::Error,
            true,
            Some(Area::ALL.len())
        )
    );
}

#[test]
fn doctor_machine_rejects_out_of_order_and_incomplete_input() {
    let mut machine = doctor::Machine::new();
    let wrong = machine.record(
        Area::Repositories,
        Observation::new(Status::Ok, "checked", "available"),
    );
    let incomplete = machine.finish();
    assert_eq!(
        (
            matches!(wrong, Err(doctor::Error::UnexpectedArea { .. })),
            matches!(incomplete, Err(doctor::Error::Incomplete(7))),
        ),
        (true, true)
    );
}
