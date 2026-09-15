use std::path::PathBuf;

use serde_json::{Value, json};

use super::Paths;

fn wire() -> Value {
    json!({
        "root": "/private/run",
        "runtime": "/private/run/runtime",
        "provider": "/private/run/copilot/session-state/sdk/extensions/sdk",
        "copilot_home": "/private/run/copilot",
        "user_home": "/private/run/home",
        "session": "/private/run/copilot/session-state/sdk"
    })
}

#[test]
fn internal_storage_grouping_preserves_the_existing_flat_wire_shape() {
    let paths: Paths = serde_json::from_value(wire()).expect("original flat paths");
    assert_eq!(
        (
            serde_json::to_value(&paths).expect("serialized paths"),
            paths.storage.session
        ),
        (
            wire(),
            PathBuf::from("/private/run/copilot/session-state/sdk")
        ),
    );
}

#[test]
fn unknown_or_missing_flat_path_fields_remain_invalid() {
    let mut unknown = wire();
    unknown["storage"] = json!({});
    let mut missing = wire();
    missing.as_object_mut().expect("object").remove("session");
    for value in [unknown, missing] {
        assert!(serde_json::from_value::<Paths>(value).is_err());
    }
}
