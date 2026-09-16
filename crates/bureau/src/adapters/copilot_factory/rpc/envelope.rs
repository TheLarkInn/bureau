use serde_json::{Map, Value, json};

use super::{Error, RpcFault};

fn invalid(message: &str) -> Error {
    Error::Protocol(message.to_owned())
}

fn reverse_id(id: &Value) -> Result<Value, Error> {
    match id {
        Value::String(_) | Value::Number(_) => Ok(id.clone()),
        _ => Err(invalid("reverse request id must be a string or number")),
    }
}

fn parameters(object: &Map<String, Value>) -> Result<Value, Error> {
    match object.get("params") {
        None => Ok(Value::Null),
        Some(value @ (Value::Array(_) | Value::Object(_))) => Ok(value.clone()),
        _ => Err(invalid("params must be an object or array when present")),
    }
}

pub(super) enum Message {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    Response {
        id: u64,
        result: Result<Value, RpcFault>,
    },
}

fn response_payload(id: u64, object: &Map<String, Value>) -> Result<Message, Error> {
    let result = match (object.get("result"), object.get("error")) {
        (Some(value), None) => Ok(value.clone()),
        (None, Some(value)) => Err(serde_json::from_value(value.clone())
            .map_err(|error| Error::Protocol(format!("malformed error object: {error}")))?),
        _ => {
            return Err(invalid(
                "response must contain exactly one of result or error",
            ));
        }
    };
    Ok(Message::Response { id, result })
}

fn call(object: &Map<String, Value>) -> Result<Message, Error> {
    if object.contains_key("result") || object.contains_key("error") {
        return Err(invalid(
            "request or notification contains a response payload",
        ));
    }
    let method = object["method"]
        .as_str()
        .ok_or_else(|| invalid("method must be a string"))?
        .to_owned();
    let params = parameters(object)?;
    match object.get("id") {
        Some(id) => Ok(Message::Request {
            id: reverse_id(id)?,
            method,
            params,
        }),
        None => Ok(Message::Notification { method, params }),
    }
}

fn response(object: &Map<String, Value>) -> Result<Message, Error> {
    if object.contains_key("params") {
        return Err(invalid("response contains request parameters"));
    }
    let id = object
        .get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid("response id must exactly match an unsigned request id"))?;
    response_payload(id, object)
}

pub(super) fn parse(value: &Value) -> Result<Message, Error> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("envelope must be an object"))?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(invalid("jsonrpc must be exactly \"2.0\""));
    }
    if object.contains_key("method") {
        return call(object);
    }
    response(object)
}

pub(super) fn request(id: u64, method: &str, params: &Value) -> Result<Value, Error> {
    let mut message = json!({"jsonrpc": "2.0", "id": id, "method": method});
    match params {
        Value::Null => {}
        Value::Array(_) | Value::Object(_) => message["params"] = params.clone(),
        _ => {
            return Err(invalid(
                "outbound params must be null, an object, or an array",
            ));
        }
    }
    Ok(message)
}

pub(super) fn reply(id: &Value, result: Result<Value, RpcFault>) -> Value {
    match result {
        Ok(value) => json!({"jsonrpc": "2.0", "id": id, "result": value}),
        Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
    }
}
