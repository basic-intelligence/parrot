use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct RequestLine {
    pub id: String,
    pub method: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsonLineError {
    pub id: Option<String>,
    pub message: String,
}

pub fn parse_request_line(line: &str) -> Result<RequestLine, JsonLineError> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err(JsonLineError {
            id: None,
            message: "empty request line".into(),
        });
    }

    let value: Value = serde_json::from_str(trimmed).map_err(|error| JsonLineError {
        id: None,
        message: format!("invalid JSON: {error}"),
    })?;

    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| JsonLineError {
            id: None,
            message: "request is missing string field `id`".into(),
        })?;

    let method = value
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| JsonLineError {
            id: Some(id.clone()),
            message: "request is missing string field `method`".into(),
        })?;

    let payload = value.get("payload").cloned().unwrap_or(Value::Null);

    Ok(RequestLine {
        id,
        method,
        payload,
    })
}

pub fn serialize_json_line(value: &Value) -> anyhow::Result<String> {
    let mut line = serde_json::to_string(value)?;
    line.push('\n');
    Ok(line)
}

pub fn success_response(id: &str, payload: Value) -> Value {
    json!({
        "id": id,
        "ok": true,
        "payload": payload
    })
}

pub fn error_response(id: &str, error: impl Into<String>) -> Value {
    json!({
        "id": id,
        "ok": false,
        "error": error.into()
    })
}

#[allow(dead_code)]
pub fn event_message(event: &str, payload: Value) -> Value {
    json!({
        "event": event,
        "payload": payload
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_request_line() {
        let request = parse_request_line(
            r#"{"id":"one","method":"initialize","payload":{"debugCleanupFailures":true}}"#,
        )
        .unwrap();

        assert_eq!(request.id, "one");
        assert_eq!(request.method, "initialize");
        assert_eq!(request.payload["debugCleanupFailures"], true);
    }

    #[test]
    fn invalid_json_returns_error_without_panicking() {
        let error = parse_request_line(r#"{"id":"one","method":"initialize""#).unwrap_err();

        assert!(error.id.is_none());
        assert!(error.message.starts_with("invalid JSON:"));
    }

    #[test]
    fn missing_method_preserves_request_id_in_error() {
        let error = parse_request_line(r#"{"id":"one","payload":{}}"#).unwrap_err();

        assert_eq!(error.id.as_deref(), Some("one"));
        assert_eq!(error.message, "request is missing string field `method`");
    }

    #[test]
    fn serializes_success_response_line() {
        let line = serialize_json_line(&success_response("one", json!({"status": "ok"}))).unwrap();

        assert_eq!(
            line,
            r#"{"id":"one","ok":true,"payload":{"status":"ok"}}"#.to_string() + "\n"
        );
    }

    #[test]
    fn serializes_error_response_line() {
        let line = serialize_json_line(&error_response("one", "not implemented")).unwrap();
        let value: Value = serde_json::from_str(line.trim()).unwrap();

        assert!(line.ends_with('\n'));
        assert_eq!(value["id"], "one");
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"], "not implemented");
    }

    #[test]
    fn serializes_event_line() {
        let line = serialize_json_line(&event_message(
            "parrot:recording-started",
            json!({"kind": "dictation"}),
        ))
        .unwrap();

        assert_eq!(
            line,
            r#"{"event":"parrot:recording-started","payload":{"kind":"dictation"}}"#.to_string()
                + "\n"
        );
    }
}
