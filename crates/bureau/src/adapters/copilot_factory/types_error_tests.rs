use serde_json::{Value, json};

use super::RpcError;

#[test]
fn raw_rpc_error_retains_numeric_and_structured_codes() {
    let data = json!({"code": "agent_factories_unavailable", "details": {"eligible": false}});
    let error: RpcError = serde_json::from_value(json!({
        "code": -32601, "message": "unavailable", "data": data, "futureField": true,
    }))
    .expect("raw error");
    assert_eq!(
        (
            error.code,
            error.message.as_str(),
            error.data_code(),
            error.data.as_ref()
        ),
        (
            -32601,
            "unavailable",
            Some("agent_factories_unavailable"),
            Some(&data)
        )
    );
}

#[test]
fn rpc_data_without_a_string_code_is_still_preserved() {
    for data in [
        Value::Null,
        json!(["detail"]),
        json!({"code": 7}),
        json!({"detail": "missing"}),
    ] {
        let error: RpcError = serde_json::from_value(json!({
            "code": -32000, "message": "failure", "data": data,
        }))
        .expect("raw error");
        assert_eq!(
            (error.data_code(), error.data.as_ref()),
            (None, Some(&data))
        );
    }
}

#[test]
fn absent_rpc_data_stays_absent() {
    let value = json!({"code": -32602, "message": "invalid params"});
    let error: RpcError = serde_json::from_value(value.clone()).expect("raw error");
    let encoded = serde_json::to_value(error).expect("encode");
    assert_eq!(encoded, value);
}

#[test]
fn missing_or_invalid_rpc_error_fields_fail() {
    for value in [
        json!({}),
        json!({"code": -32601}),
        json!({"message": "failure"}),
        json!({"code": 0.5, "message": "failure"}),
        json!({"code": "-32601", "message": "failure"}),
        json!({"code": -32601, "message": null}),
    ] {
        assert!(serde_json::from_value::<RpcError>(value).is_err());
    }
}
