//! JSON-RPC 2.0 over UTF-8 JSON Lines framing and `mechcore.error` mapping.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::cli::{Failure, Kind};

/// JSON-RPC 2.0 request/response identifier (`u64 | string`, never `null`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum RequestId {
    Number(u64),
    String(String),
}

/// JSON-RPC 2.0 error codes defined in `docs/spec/mechcore/rpc.md`.
pub(crate) mod code {
    pub(crate) const PARSE_ERROR: i32 = -32700;
    pub(crate) const INVALID_REQUEST: i32 = -32600;
    pub(crate) const METHOD_NOT_FOUND: i32 = -32601;
    pub(crate) const INVALID_PARAMS: i32 = -32602;
    pub(crate) const RULE_REFUSED: i32 = -32001;
    pub(crate) const REQUEST_CANCELLED: i32 = -32800;
    pub(crate) const UNAVAILABLE: i32 = -32002;
    pub(crate) const INTERNAL_ERROR: i32 = -32603;
}

/// Unified RPC error carrying its JSON-RPC code and embedded `mechcore.error`.
#[derive(Debug)]
pub(crate) struct RpcError {
    pub(crate) code: i32,
    pub(crate) operation: String,
    pub(crate) failure: Failure,
}

impl RpcError {
    pub(crate) fn new(code: i32, operation: impl Into<String>, failure: Failure) -> Self {
        Self {
            code,
            operation: operation.into(),
            failure,
        }
    }

    pub(crate) fn parse_error(operation: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(code::PARSE_ERROR, operation, Failure::usage(reason))
    }

    pub(crate) fn invalid_request(operation: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(code::INVALID_REQUEST, operation, Failure::usage(reason))
    }

    pub(crate) fn method_not_found(operation: impl Into<String>, method: &str) -> Self {
        Self::new(
            code::METHOD_NOT_FOUND,
            operation,
            Failure::usage(format!("unknown JSON-RPC method {method:?}")),
        )
    }

    pub(crate) fn invalid_params(operation: impl Into<String>, failure: Failure) -> Self {
        Self::new(code::INVALID_PARAMS, operation, failure)
    }

    pub(crate) fn cancelled(operation: impl Into<String>) -> Self {
        Self::new(
            code::REQUEST_CANCELLED,
            operation,
            Failure::refused("request cancelled"),
        )
    }

    pub(crate) fn from_failure(operation: impl Into<String>, failure: Failure) -> Self {
        let code = match failure.kind() {
            Kind::Usage => code::INVALID_PARAMS,
            Kind::Refused => code::RULE_REFUSED,
            Kind::Unavailable => code::UNAVAILABLE,
            Kind::Failed => code::INTERNAL_ERROR,
        };
        Self::new(code, operation, failure)
    }

    pub(crate) fn to_response(&self, id: Option<&RequestId>) -> Value {
        let id_value = id.map_or(Value::Null, |id| json!(id));
        json!({
            "jsonrpc": "2.0",
            "id": id_value,
            "error": {
                "code": self.code,
                "message": self.failure.reason(),
                "data": self.failure.object(&self.operation),
            }
        })
    }
}

pub(crate) fn ok_response(id: &RequestId, result: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

/// One parsed JSON-RPC 2.0 message from a single line or batch array element.
#[derive(Debug)]
pub(crate) enum IncomingMessage {
    Request {
        id: RequestId,
        method: String,
        params: Option<Value>,
    },
    Notification {
        method: String,
        params: Option<Value>,
    },
    Response {
        id: RequestId,
        result: Option<Value>,
        error: Option<Value>,
    },
    Invalid {
        id: Option<RequestId>,
        error: RpcError,
    },
}

/// Result of parsing a raw JSONL frame (`None` on empty/whitespace-only line).
pub(crate) enum ParsedFrame {
    Empty,
    Items(Vec<IncomingMessage>),
}

/// Parses raw UTF-8 line bytes into one or more [`IncomingMessage`] items.
pub(crate) fn parse_line(raw: &[u8]) -> ParsedFrame {
    let text = match std::str::from_utf8(raw) {
        Ok(text) => text,
        Err(error) => {
            return ParsedFrame::Items(vec![IncomingMessage::Invalid {
                id: None,
                error: RpcError::parse_error("rpc", format!("invalid UTF-8: {error}")),
            }]);
        }
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return ParsedFrame::Empty;
    }
    let value = match serde_json::from_str::<Value>(trimmed) {
        Ok(value) => value,
        Err(error) => {
            return ParsedFrame::Items(vec![IncomingMessage::Invalid {
                id: None,
                error: RpcError::parse_error("rpc", format!("invalid JSON: {error}")),
            }]);
        }
    };
    match value {
        Value::Array(items) if items.is_empty() => {
            ParsedFrame::Items(vec![IncomingMessage::Invalid {
                id: None,
                error: RpcError::invalid_request("rpc", "empty JSON-RPC batch"),
            }])
        }
        Value::Array(items) => ParsedFrame::Items(items.into_iter().map(classify_item).collect()),
        other => ParsedFrame::Items(vec![classify_item(other)]),
    }
}

fn classify_item(value: Value) -> IncomingMessage {
    let Value::Object(mut obj) = value else {
        return IncomingMessage::Invalid {
            id: None,
            error: RpcError::invalid_request("rpc", "JSON-RPC message must be a JSON object"),
        };
    };

    let has_id = obj.contains_key("id");
    let parsed_id = if let Some(raw_id) = obj.remove("id") {
        match parse_request_id(&raw_id) {
            Some(id) => Some(id),
            None => {
                return IncomingMessage::Invalid {
                    id: None,
                    error: RpcError::invalid_request(
                        "rpc",
                        "JSON-RPC id must be a non-negative integer or string (never null)",
                    ),
                };
            }
        }
    } else {
        None
    };

    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return IncomingMessage::Invalid {
            id: parsed_id,
            error: RpcError::invalid_request("rpc", "missing or invalid \"jsonrpc\": \"2.0\""),
        };
    }
    let _ = obj.remove("jsonrpc");

    if obj.contains_key("method") {
        classify_request_or_notification(&mut obj, has_id, parsed_id)
    } else {
        classify_response(obj, parsed_id)
    }
}

fn classify_request_or_notification(
    obj: &mut Map<String, Value>,
    has_id: bool,
    parsed_id: Option<RequestId>,
) -> IncomingMessage {
    let Some(Value::String(method)) = obj.remove("method") else {
        return IncomingMessage::Invalid {
            id: parsed_id,
            error: RpcError::invalid_request("rpc", "JSON-RPC \"method\" must be a string"),
        };
    };
    let params = match obj.remove("params") {
        None | Some(Value::Null) => None,
        Some(value @ Value::Object(_)) => Some(value),
        Some(_) => {
            return IncomingMessage::Invalid {
                id: parsed_id,
                error: RpcError::invalid_params(
                    method,
                    Failure::usage("JSON-RPC \"params\" must be an object"),
                ),
            };
        }
    };
    if let Some((unknown, _)) = obj.iter().next() {
        return IncomingMessage::Invalid {
            id: parsed_id,
            error: RpcError::invalid_request(
                method,
                format!("unexpected field {unknown:?} in JSON-RPC request"),
            ),
        };
    }
    if has_id {
        IncomingMessage::Request {
            id: parsed_id.expect("has_id guarantees parsed_id"),
            method,
            params,
        }
    } else {
        IncomingMessage::Notification { method, params }
    }
}

fn classify_response(mut obj: Map<String, Value>, parsed_id: Option<RequestId>) -> IncomingMessage {
    let Some(id) = parsed_id else {
        return IncomingMessage::Invalid {
            id: None,
            error: RpcError::invalid_request("rpc", "JSON-RPC response must carry a valid id"),
        };
    };
    let result = obj.remove("result");
    let error = obj.remove("error");
    if result.is_some() == error.is_some() {
        return IncomingMessage::Invalid {
            id: Some(id),
            error: RpcError::invalid_request(
                "rpc",
                "JSON-RPC response must carry exactly one of \"result\" or \"error\"",
            ),
        };
    }
    if let Some((unknown, _)) = obj.iter().next() {
        return IncomingMessage::Invalid {
            id: Some(id),
            error: RpcError::invalid_request(
                "rpc",
                format!("unexpected field {unknown:?} in JSON-RPC response"),
            ),
        };
    }
    IncomingMessage::Response { id, result, error }
}

fn parse_request_id(value: &Value) -> Option<RequestId> {
    match value {
        Value::Number(n) => n.as_u64().map(RequestId::Number),
        Value::String(s) => Some(RequestId::String(s.clone())),
        _ => None,
    }
}
