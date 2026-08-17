//! Conservative local repair discovery and confirmed execution.

mod discover;

use crate::cli::out;
use std::io::{BufRead as _, Write as _};

fn confirmed() -> anyhow::Result<bool> {
    eprint!("Apply this repair plan? Type `yes`: ");
    std::io::stderr().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(answer.trim().eq_ignore_ascii_case("yes"))
}

pub(super) fn run(checkout: bool, config: bool) -> anyhow::Result<i32> {
    let home = bureau::home::Home::discover()?;
    let _maintenance = bureau::maintenance::exclusive(home.layout().root())?;
    super::migrate::recover_pending(home.layout(), None)?;
    let candidates = discover::candidates(home.layout(), checkout, config)?;
    let plan = bureau::repair::plan(candidates);
    out::line(format_args!("{}", serde_json::to_string_pretty(&plan)?));
    if !confirmed()? {
        out::error(format_args!("repair declined; no changes applied"));
        return Ok(2);
    }
    let mut effects = bureau::repair::LocalEffects::new(home.layout());
    let summary = bureau::repair::run(plan, bureau::repair::Confirmation::Approve, &mut effects)?;
    out::line(format_args!(
        "repair complete: {} action(s)",
        summary.applied
    ));
    Ok(0)
}
