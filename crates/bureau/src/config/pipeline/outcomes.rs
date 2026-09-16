use std::collections::BTreeMap;

/// The four outcomes a `decision` step's `on` must cover (kebab-case).
const OUTCOMES: [&str; 4] = ["success", "failure", "blocked", "no-work"];

pub(super) fn check_missing(on: &BTreeMap<String, String>, errors: &mut Vec<String>) {
    for outcome in OUTCOMES {
        if !on.contains_key(outcome) {
            errors.push(format!("`on` is missing a `{outcome}` branch"));
        }
    }
}

pub(super) fn check_unknown(on: &BTreeMap<String, String>, errors: &mut Vec<String>) {
    for key in on.keys() {
        if !OUTCOMES.contains(&key.as_str()) {
            errors.push(format!("`on` has unknown outcome `{key}`"));
        }
    }
}
