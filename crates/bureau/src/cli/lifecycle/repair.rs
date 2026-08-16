//! Conservative local repair discovery and confirmed execution.

mod discover;

use std::io::{BufRead as _, Write as _};

pub(super) fn run(checkout: bool, config: bool) -> anyhow::Result<i32> {
    let home = bureau::home::Home::discover()?;
    let _maintenance = bureau::maintenance::exclusive(home.layout().root())?;
    super::migrate::recover_pending(home.layout(), None)?;
    let candidates = discover::candidates(home.layout(), checkout, config)?;
    let plan = bureau::repair::plan(candidates);
    println!("{}", serde_json::to_string_pretty(&plan)?);
    if !confirmed()? {
        eprintln!("repair declined; no changes applied");
        return Ok(2);
    }
    let mut effects = bureau::repair::LocalEffects::new(home.layout());
    let summary = bureau::repair::run(plan, bureau::repair::Confirmation::Approve, &mut effects)?;
    println!("repair complete: {} action(s)", summary.applied);
    Ok(0)
}

fn confirmed() -> anyhow::Result<bool> {
    eprint!("Apply this repair plan? Type `yes`: ");
    std::io::stderr().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(answer.trim().eq_ignore_ascii_case("yes"))
}
