//! `watch`: a live, read-only terminal dashboard over local bureau
//! state. Piped (not a terminal), it prints one snapshot and exits 0.

mod tui;

use std::io::IsTerminal as _;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use bureau::watch::{self, Roots};

use super::out;

/// Event lines in the detail pane.
const DETAIL_LINES: usize = 16;

/// The process clock boundary: milliseconds since the Unix epoch. The
/// clock function is bound first so this helper stays the one place
/// naming the process clock.
fn now_millis() -> u64 {
    let now = SystemTime::now;
    now().duration_since(UNIX_EPOCH).map_or(0, |since| {
        u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
    })
}

/// Explicit roots win; the rest default from the discovered home (the
/// home directory itself need not exist — every read tolerates that).
fn roots(
    runs: Option<PathBuf>,
    state: Option<PathBuf>,
    config_cache: Option<PathBuf>,
) -> anyhow::Result<Roots> {
    let (runs, state, config_cache) = match (runs, state, config_cache) {
        (Some(runs), Some(state), Some(config_cache)) => {
            return Ok(Roots {
                runs,
                state,
                config_cache,
            });
        }
        values => values,
    };
    let home = bureau::home::Home::discover()?;
    let layout = home.layout();
    Ok(Roots {
        runs: runs.unwrap_or_else(|| layout.runs().to_path_buf()),
        state: state.unwrap_or_else(|| layout.state_db().to_path_buf()),
        config_cache: config_cache.unwrap_or_else(|| layout.config_cache().to_path_buf()),
    })
}

/// `watch`: the dashboard on a terminal, one snapshot when piped.
///
/// # Errors
/// Propagates home-discovery and terminal failures.
pub fn run(
    runs: Option<PathBuf>,
    state: Option<PathBuf>,
    config_cache: Option<PathBuf>,
) -> anyhow::Result<i32> {
    let roots = roots(runs, state, config_cache)?;
    if !std::io::stdout().is_terminal() {
        let frame = watch::load(&roots, None, DETAIL_LINES, now_millis());
        for line in watch::render_plain(&frame) {
            out::line(format_args!("{line}"));
        }
        return Ok(0);
    }
    tui::run(&roots)
}
