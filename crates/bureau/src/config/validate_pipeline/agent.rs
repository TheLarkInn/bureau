use std::path::Path;

use super::super::{AdapterKind, Config, Permission, Role, StepDef, step_err};
use crate::ConfigError;

fn check_fixture(
    errors: &mut Vec<ConfigError>,
    name: &str,
    step: &StepDef,
    role: &Role,
    path: &Path,
) {
    let Some(fixture) = step.fixture.as_deref() else {
        return;
    };
    let mut err = |detail: &str| step_err(errors, path, name, &step.name, detail);
    if role.adapter != AdapterKind::Fake {
        err("`fixture` requires a role with the `fake` adapter");
    }
    if !Path::new(fixture).is_absolute() {
        err("`fixture` must be an absolute path");
    }
}

fn check_factory(
    errors: &mut Vec<ConfigError>,
    name: &str,
    step: &StepDef,
    role: &Role,
    path: &Path,
) {
    if step.copilot_factory.is_none() {
        return;
    }
    let mut err = |detail: &str| step_err(errors, path, name, &step.name, detail);
    if role.adapter != AdapterKind::Copilot {
        err("`copilot_factory` requires a role with the `copilot` adapter");
    }
    if !role.permissions.contains(&Permission::ModelInvoke) {
        err("`copilot_factory` requires `model:invoke` on its role");
    }
}

pub(super) fn check(
    errors: &mut Vec<ConfigError>,
    config: &Config,
    name: &str,
    step: &StepDef,
    path: &Path,
) {
    let Some(role_name) = step.role.as_deref() else {
        return;
    };
    let Some(role) = config.roles.get(role_name) else {
        step_err(
            errors,
            path,
            name,
            &step.name,
            &format!("references unknown role `{role_name}`"),
        );
        return;
    };
    check_fixture(errors, name, step, role, path);
    check_factory(errors, name, step, role, path);
}
