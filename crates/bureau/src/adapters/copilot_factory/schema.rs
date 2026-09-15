//! Complete pinned argument-schema validation without external resource loading.

use boon::{Compiler, Draft, Schemas, UrlLoader};
use serde_json::Value;

use super::SetupError;

const RESOURCE: &str = "https://bureau.invalid/factory-args.json";
const DIALECTS: [&str; 5] = [
    "http://json-schema.org/draft-04/schema",
    "http://json-schema.org/draft-06/schema",
    "http://json-schema.org/draft-07/schema",
    "https://json-schema.org/draft/2019-09/schema",
    "https://json-schema.org/draft/2020-12/schema",
];

struct NoExternalResources;

impl UrlLoader for NoExternalResources {
    fn load(&self, _url: &str) -> Result<Value, Box<dyn std::error::Error>> {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "external factory schema resources are forbidden",
        )
        .into())
    }
}

fn check_dialect(schema: &Value) -> Result<(), String> {
    if !(schema.is_object() || schema.is_boolean()) {
        return Err("factory argsSchema must be a JSON Schema object or boolean".to_owned());
    }
    let Some(dialect) = schema.get("$schema") else {
        return Ok(());
    };
    let supported = dialect
        .as_str()
        .is_some_and(|value| DIALECTS.contains(&value.trim_end_matches('#')));
    if supported {
        Ok(())
    } else {
        Err("factory argsSchema names an unsupported or invalid $schema dialect".to_owned())
    }
}

fn invalid_schema(error: impl std::fmt::Display) -> SetupError {
    SetupError::Schema(format!("invalid factory argsSchema: {error:#}"))
}

/// Validates every declared argument constraint using an explicit schema dialect.
///
/// Absent `$schema` means draft 7. Internal references are allowed; external files
/// and URLs are never loaded. No runtime-subset fallback is used.
///
/// # Errors
/// Rejects unsupported dialects, malformed schemas, unresolved references, and invalid arguments.
pub fn validate(schema: &Value, arguments: &Value) -> Result<(), SetupError> {
    check_dialect(schema).map_err(SetupError::Schema)?;
    let mut compiler = Compiler::new();
    compiler.set_default_draft(Draft::V7);
    compiler.use_loader(Box::new(NoExternalResources));
    compiler
        .add_resource(RESOURCE, schema.clone())
        .map_err(invalid_schema)?;
    let mut schemas = Schemas::new();
    let index = compiler
        .compile(RESOURCE, &mut schemas)
        .map_err(invalid_schema)?;
    schemas
        .validate(arguments, index)
        .map_err(|error| SetupError::Arguments(format!("invalid factory arguments: {error}")))
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
