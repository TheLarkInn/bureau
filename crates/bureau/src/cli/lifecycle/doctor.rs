//! Read-only local doctor command.

pub(super) fn run(json: bool) -> anyhow::Result<i32> {
    let home = bureau::home::Home::discover()?;
    let effects = bureau::doctor::LocalEffects::new(home.layout());
    let report = bureau::doctor::run(&effects);
    if json {
        println!("{}", report.json()?);
    } else {
        println!("{}", report.human());
    }
    Ok(exit_code(report.status()))
}

const fn exit_code(status: bureau::doctor::Status) -> i32 {
    match status {
        bureau::doctor::Status::Error => 1,
        bureau::doctor::Status::Ok | bureau::doctor::Status::Warning => 0,
    }
}
