//! Read-only local doctor command.

mod identity;

use crate::cli::out;

const fn exit_code(status: bureau::doctor::Status) -> i32 {
    match status {
        bureau::doctor::Status::Error => 1,
        bureau::doctor::Status::Ok | bureau::doctor::Status::Warning => 0,
    }
}

pub(super) async fn run(json: bool) -> anyhow::Result<i32> {
    let home = bureau::home::Home::discover()?;
    let effects = bureau::doctor::LocalEffects::new(home.layout());
    let identities = identity::verify(&effects).await;
    let report = bureau::doctor::run(&effects.identities(identities));
    if json {
        out::line(format_args!("{}", report.json()?));
    } else {
        out::line(format_args!("{}", report.human()));
    }
    Ok(exit_code(report.status()))
}
