//! A Model Context Protocol server on standard input and output: JSON-RPC 2.0, one JSON
//! object per line. The tools give the same text as the commands they stand for.

use crate::app::dispatch;
use crate::cli::{Command, FindArgs, RecallArgs, RetainArgs, DEFAULT_FIND_K, DEFAULT_PACK_BUDGET};
use crate::ctx::Ctx;
use crate::fail::Fail;
use lxr_memory::{Budget, Kind};
use lxr_rag::Mode;
use serde_json::{json, Map, Value};
use std::io::{self, BufRead, Write};

/// Protocol versions the server speaks, newest first.
const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// A JSON-RPC error: its code and message.
type RpcError = (i64, String);

/// Serves requests from standard input until it closes.
pub fn serve(ctx: &mut Ctx) {
    let stdin = io::stdin();
    let stdout = io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handle(ctx, &line) {
            let mut out = stdout.lock();
            if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
                break;
            }
        }
    }
}

fn reply(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn failure(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Answers one line. `None` for a notification, which has no answer.
fn handle(ctx: &mut Ctx, line: &str) -> Option<Value> {
    let message: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(_) => return Some(failure(Value::Null, -32700, "parse error")),
    };
    let Some(object) = message.as_object() else {
        return Some(failure(Value::Null, -32600, "invalid request"));
    };
    let id = object.get("id").cloned();
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        // No method: a response from the client is ignored, anything else is invalid.
        let is_response = object.contains_key("result") || object.contains_key("error");
        return id.filter(|_| !is_response).map(|id| failure(id, -32600, "invalid request"));
    };
    // A message without an id is a notification and gets no answer.
    let id = id?;
    let params = object.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
            let version = VERSIONS.iter().find(|v| **v == asked).unwrap_or(&VERSIONS[0]);
            reply(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "pgr", "version": env!("CARGO_PKG_VERSION") },
                }),
            )
        }
        "ping" => reply(id, json!({})),
        "tools/list" => reply(id, json!({ "tools": tools() })),
        "tools/call" => match call(ctx, &params) {
            Ok(result) => reply(id, result),
            Err((code, text)) => failure(id, code, &text),
        },
        _ => failure(id, -32601, "method not found"),
    })
}

/// The tools and their argument schemas.
fn tools() -> Value {
    json!([
        {
            "name": "ask",
            "description": "Answer several code questions at once. Each question is KIND:WHAT, with KIND one of def, callers, about, affected, explain, find, recall, path (path takes A>B).",
            "inputSchema": {
                "type": "object",
                "properties": { "questions": { "type": "array", "items": { "type": "string" }, "minItems": 1 } },
                "required": ["questions"]
            }
        },
        {
            "name": "find",
            "description": "Hybrid code search: the definitions that best match the words.",
            "inputSchema": {
                "type": "object",
                "properties": { "query": { "type": "string" }, "limit": { "type": "integer", "minimum": 1 } },
                "required": ["query"]
            }
        },
        {
            "name": "recall",
            "description": "Recall the notes that best match the words.",
            "inputSchema": {
                "type": "object",
                "properties": { "query": { "type": "string" }, "limit": { "type": "integer", "minimum": 1 } },
                "required": ["query"]
            }
        },
        {
            "name": "retain",
            "description": "Keep a note for later sessions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": { "type": "string" },
                    "kind": { "type": "string", "enum": ["world", "experience", "directive"] },
                    "tags": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["text"]
            }
        },
        {
            "name": "pack",
            "description": "One answer for a task: search hits, a graph walk and matching notes, within a token budget.",
            "inputSchema": {
                "type": "object",
                "properties": { "task": { "type": "string" }, "budget": { "type": "integer", "minimum": 1 } },
                "required": ["task"]
            }
        }
    ])
}

fn invalid(text: impl Into<String>) -> RpcError {
    (-32602, text.into())
}

fn text_arg(args: &Map<String, Value>, name: &str) -> Result<String, RpcError> {
    match args.get(name) {
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err(invalid(format!("'{name}' must be a string"))),
    }
}

fn count_arg(args: &Map<String, Value>, name: &str) -> Result<Option<usize>, RpcError> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => match value.as_u64() {
            Some(n) if n >= 1 => Ok(Some(n as usize)),
            _ => Err(invalid(format!("'{name}' must be a whole number of at least 1"))),
        },
    }
}

fn list_arg(args: &Map<String, Value>, name: &str) -> Result<Option<Vec<String>>, RpcError> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| item.as_str().map(str::to_string).ok_or_else(|| invalid(format!("'{name}' must hold strings"))))
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => Err(invalid(format!("'{name}' must be an array of strings"))),
    }
}

/// Turns the arguments of a tool call into a command.
fn command_for(name: &str, args: &Map<String, Value>) -> Result<Command, RpcError> {
    Ok(match name {
        "ask" => {
            let questions = list_arg(args, "questions")?.ok_or_else(|| invalid("'questions' is required"))?;
            if questions.is_empty() {
                return Err(invalid("'questions' must hold at least one question"));
            }
            Command::Ask { questions }
        }
        "find" => Command::Find(FindArgs {
            query: text_arg(args, "query")?,
            k: count_arg(args, "limit")?.unwrap_or(DEFAULT_FIND_K),
            mode: Mode::Hybrid,
        }),
        "recall" => {
            let mut recall = RecallArgs::new(text_arg(args, "query")?);
            if let Some(limit) = count_arg(args, "limit")? {
                recall.k = limit;
            }
            recall.budget = Budget::Mid;
            Command::Recall(recall)
        }
        "retain" => {
            let kind = match args.get("kind") {
                None | Some(Value::Null) => Kind::World,
                Some(Value::String(word)) => match Kind::from_name(word) {
                    Some(k @ (Kind::World | Kind::Experience | Kind::Directive)) => k,
                    _ => return Err(invalid("'kind' must be world, experience or directive")),
                },
                Some(_) => return Err(invalid("'kind' must be a string")),
            };
            Command::Retain(RetainArgs {
                text: text_arg(args, "text")?,
                kind,
                tags: list_arg(args, "tags")?.unwrap_or_default(),
                context: String::new(),
                when: None,
                whole: false,
            })
        }
        "pack" => Command::Pack {
            task: text_arg(args, "task")?,
            budget: count_arg(args, "budget")?.unwrap_or(DEFAULT_PACK_BUDGET),
        },
        other => return Err(invalid(format!("unknown tool '{other}'"))),
    })
}

/// Runs a `tools/call` request.
fn call(ctx: &mut Ctx, params: &Value) -> Result<Value, RpcError> {
    let name = params.get("name").and_then(Value::as_str).ok_or_else(|| invalid("'name' is required"))?;
    let empty = Map::new();
    let args = match params.get("arguments") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(map)) => map,
        Some(_) => return Err(invalid("'arguments' must be an object")),
    };
    let command = command_for(name, args)?;
    ctx.begin_request();
    let (text, is_error) = match dispatch(ctx, &command) {
        Ok(text) => (text, false),
        Err(Fail::Off(message)) => (format!("pgr: {message}\n"), false),
        Err(other) => (format!("{}\n", other.text()), true),
    };
    Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_arguments_become_commands() {
        let args = json!({ "query": "token expiry", "limit": 3 });
        match command_for("find", args.as_object().unwrap()).unwrap() {
            Command::Find(f) => {
                assert_eq!(f.query, "token expiry");
                assert_eq!(f.k, 3);
            }
            other => panic!("{other:?}"),
        }
        let bad = json!({ "limit": 0, "query": "x" });
        assert!(command_for("find", bad.as_object().unwrap()).is_err());
        assert!(command_for("nope", &Map::new()).is_err());
    }
}
