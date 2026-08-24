//! `bureau validate` — checks a config directory and reports every error in
//! one pass, as prose or as one JSON document.
//!
//! The JSON form carries the loaded `Config` alongside the errors so a reader
//! outside this process never re-implements either rule set: not validation,
//! and not loading, which is also `deny_unknown_fields`, stem-must-equal-name,
//! and a non-recursive `read_dir`.

use std::path::Path;

use bureau::ConfigError;
use bureau::config::Config;

use super::out;

#[derive(serde::Serialize)]
struct AgentIdentity<'a> {
    configured: &'a str,
    resolved: String,
}

#[derive(serde::Serialize)]
struct Document<'a> {
    ok: bool,
    dir: String,
    errors: &'a [ConfigError],
    config: Option<&'a Config>,
    agents: std::collections::BTreeMap<&'a str, AgentIdentity<'a>>,
}

fn agents(config: Option<&Config>) -> std::collections::BTreeMap<&str, AgentIdentity<'_>> {
    config
        .into_iter()
        .flat_map(|config| &config.roles)
        .map(|(name, role)| {
            (
                name.as_str(),
                AgentIdentity {
                    configured: &role.agent,
                    resolved: bureau::adapters::expected_agent(role),
                },
            )
        })
        .collect()
}

fn report_errors(errors: &[ConfigError]) -> i32 {
    for error in errors {
        out::error(format_args!("{error}"));
    }
    out::error(format_args!("{} config error(s)", errors.len()));
    1
}

fn human_report(dir: &Path) -> i32 {
    match Config::load(dir) {
        Ok(config) => {
            out::line(format_args!(
                "config ok: {} repos, {} roles, {} assignments",
                config.repos.len(),
                config.roles.len(),
                config.assignments.len()
            ));
            0
        }
        Err(errors) => report_errors(&errors),
    }
}

fn json_report(dir: &Path) -> anyhow::Result<i32> {
    let loaded = Config::load(dir);
    let errors = loaded.as_ref().err().map_or(&[][..], Vec::as_slice);
    let document = Document {
        ok: errors.is_empty(),
        dir: dir.display().to_string(),
        errors,
        config: loaded.as_ref().ok(),
        agents: agents(loaded.as_ref().ok()),
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
