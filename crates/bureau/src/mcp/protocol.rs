use serde_json::{Map, Value, json};

pub(super) const INVALID_REQUEST: i64 = -32600;
pub(super) const METHOD_NOT_FOUND: i64 = -32601;
pub(super) const INVALID_PARAMS: i64 = -32602;

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message}
    })
}

fn invalid_request(id: &Value) -> Value {
    error(id, INVALID_REQUEST, "invalid request")
}

fn success(id: &Value, result: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

pub(super) struct Failure {
    code: i64,
    message: String,
}

impl Failure {
    pub(super) fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: INVALID_PARAMS,
            message: message.into(),
        }
    }

    pub(super) fn method_not_found(method: &str) -> Self {
        Self {
            code: METHOD_NOT_FOUND,
            message: format!("method {method:?} was not found"),
        }
    }
}

fn failure(id: &Value, failure: &Failure) -> Value {
    error(id, failure.code, &failure.message)
}

pub(super) struct Request {
    pub(super) method: String,
    pub(super) params: Option<Value>,
    id: Option<Value>,
}

impl Request {
    pub(super) fn complete(&self, result: Result<Value, Failure>) -> Option<Value> {
        let id = self.id.as_ref()?;
        Some(match result {
            Ok(value) => success(id, &value),
            Err(error) => failure(id, &error),
        })
    }
}

fn valid_id(value: &Value) -> bool {
    value.is_null() || value.is_string() || value.is_number()
}

fn valid_params(value: &Value) -> bool {
    value.is_object() || value.is_array()
}

fn id_or_null(object: &Map<String, Value>) -> Value {
    object
        .get("id")
        .filter(|value| valid_id(value))
        .cloned()
        .unwrap_or(Value::Null)
}

fn validate_version(object: &Map<String, Value>) -> Result<(), Value> {
    if object.get("jsonrpc").and_then(Value::as_str) == Some("2.0") {
        return Ok(());
    }
    Err(invalid_request(&id_or_null(object)))
}

fn validate_id(object: &Map<String, Value>) -> Result<(), Value> {
    if object.get("id").is_none_or(valid_id) {
        return Ok(());
    }
    Err(invalid_request(&Value::Null))
}

fn validate_params(object: &Map<String, Value>) -> Result<(), Value> {
    if object.get("params").is_none_or(valid_params) {
        return Ok(());
    }
    Err(invalid_request(&id_or_null(object)))
}

fn method(object: &Map<String, Value>) -> Result<String, Value> {
    object
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid_request(&id_or_null(object)))
}

fn parse_object(object: &Map<String, Value>) -> Result<Request, Value> {
    validate_version(object)?;
    validate_id(object)?;
    validate_params(object)?;
    let method = method(object)?;
    Ok(Request {
        method,
        params: object.get("params").cloned(),
        id: object.get("id").cloned(),
    })
}

pub(super) fn parse(line: &[u8]) -> Result<Request, Value> {
    let value: Value =
        serde_json::from_slice(line).map_err(|_| error(&Value::Null, -32700, "parse error"))?;
    let object = value
        .as_object()
        .ok_or_else(|| invalid_request(&Value::Null))?;
    parse_object(object)
}
