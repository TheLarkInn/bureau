//! `bureau validate` — checks a config directory and reports every error in
//! one pass, as prose or as one JSON document.
//!
//! The JSON form carries the loaded `Config` alongside the errors so a reader
//! outside this process never re-implements either rule set: not validation,
//! and not loading, which is also `deny_unknown_fields`, stem-must-equal-name,
//! and a non-recursive `read_dir`.

use std::collections::BTreeMap;
use std::path::Path;

use bureau::ConfigError;
use bureau::config::{Config, validate_identities};

use super::out;

#[derive(serde::Serialize)]
struct Document<'a> {
    ok: bool,
    dir: String,
    errors: &'a [ConfigError],
    config: Option<&'a Config>,
}

fn report_errors(errors: &[ConfigError]) -> i32 {
    for error in errors {
        out::error(format_args!("{error}"));
    }
    out::error(format_args!("{} config error(s)", errors.len()));
    1
}

/// Identity declarations from this machine's settings, when it has any.
/// `validate` also runs against a bare config checkout, so an absent or
/// unreadable settings file simply declares nothing.
fn declared_identities() -> BTreeMap<String, String> {
    bureau::home::Home::discover()
        .ok()
        .and_then(|home| bureau::setup::load_settings(home.layout().settings()).ok())
        .as_ref()
        .map_or_else(BTreeMap::new, bureau::setup::Settings::declared_identities)
}

/// The loaded config and every error in one pass: the config repo's own
/// rules, then the settings-side identity declarations it must support.
fn loaded(dir: &Path) -> (Option<Config>, Vec<ConfigError>) {
    match Config::load(dir) {
        Ok(config) => {
            let errors = validate_identities(&config, &declared_identities());
            (Some(config), errors)
        }
        Err(errors) => (None, errors),
    }
}

fn human_report(dir: &Path) -> i32 {
    let (config, errors) = loaded(dir);
    let Some(config) = config.filter(|_| errors.is_empty()) else {
        return report_errors(&errors);
    };
    out::line(format_args!(
        "config ok: {} repos, {} roles, {} assignments",
        config.repos.len(),
        config.roles.len(),
        config.assignments.len()
    ));
    0
}

fn json_report(dir: &Path) -> anyhow::Result<i32> {
    let (config, errors) = loaded(dir);
    let document = Document {
        ok: errors.is_empty(),
        dir: dir.display().to_string(),
        errors: &errors,
        config: config.as_ref(),
    };
    out::line(format_args!("{}", serde_json::to_string(&document)?));
    Ok(i32::from(!errors.is_empty()))
}

/// Runs the verb, returning the process exit code.
///
/// # Errors
/// Returns an error only if the JSON document fails to serialize.
pub fn run(dir: &Path, json: bool) -> anyhow::Result<i32> {
    if json {
        return json_report(dir);
    }
    Ok(human_report(dir))
}
