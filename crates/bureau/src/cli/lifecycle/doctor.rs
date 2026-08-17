//! Read-only local doctor command.

use crate::cli::out;

const fn exit_code(status: bureau::doctor::Status) -> i32 {
    match status {
        bureau::doctor::Status::Error => 1,
        bureau::doctor::Status::Ok | bureau::doctor::Status::Warning => 0,
    }
}

pub(super) fn run(json: bool) -> anyhow::Result<i32> {
    let home = bureau::home::Home::discover()?;
    let effects = bureau::doctor::LocalEffects::new(home.layout());
    let report = bureau::doctor::run(&effects);
    if json {
        out::line(format_args!("{}", report.json()?));
    } else {
        out::line(format_args!("{}", report.human()));
    }
    Ok(exit_code(report.status()))
}
