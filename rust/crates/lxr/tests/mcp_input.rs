//! What the MCP server does with input that is too large, not UTF-8, or not a request.

mod common;
use common::Env;
use serde_json::{json, Value};
use std::io::Write;
use std::process::Stdio;

/// Sends the bytes on standard input, closes it, and gives the answers (one JSON value per line).
fn talk_bytes(env: &Env, input: &[u8]) -> Vec<Value> {
    let mut cmd = env.command();
    cmd.arg("--root").arg(&env.proj).arg("mcp");
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let bytes = input.to_vec();
    // Written from a thread: the server must answer while it is still being fed.
    let feeder = std::thread::spawn(move || {
        let _ = stdin.write_all(&bytes);
    });
    let out = child.wait_with_output().unwrap();
    feeder.join().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON ({e}): {l}")))
        .collect()
}

fn ping(id: u64) -> String {
    format!("{}\n", json!({"jsonrpc": "2.0", "id": id, "method": "ping"}))
}

#[test]
fn a_line_over_the_limit_is_refused_and_the_server_goes_on() {
    let env = Env::with_project();
    let mut input = Vec::new();
    input.extend(std::iter::repeat(b'x').take(3 * 1024 * 1024));
    input.push(b'\n');
    input.extend(ping(7).as_bytes());
    let answers = talk_bytes(&env, &input);
    assert_eq!(answers.len(), 2, "{answers:?}");
    assert_eq!(answers[0]["error"]["code"], -32600, "{}", answers[0]);
    assert_eq!(answers[0]["id"], Value::Null);
    assert!(answers[0]["error"]["message"].as_str().unwrap().contains("too long"), "{}", answers[0]);
    assert_eq!(answers[1]["id"], 7);
    assert_eq!(answers[1]["result"], json!({}));
}

#[test]
fn a_long_line_that_never_ends_in_a_line_break_is_refused_at_the_end_of_input() {
    let env = Env::with_project();
    let input = vec![b'y'; 2 * 1024 * 1024];
    let answers = talk_bytes(&env, &input);
    assert_eq!(answers.len(), 1, "{answers:?}");
    assert_eq!(answers[0]["error"]["code"], -32600);
}

#[test]
fn bytes_that_are_not_utf8_get_a_parse_error_and_the_server_goes_on() {
    let env = Env::with_project();
    let mut input = vec![0xFF, 0xFE, b'\n'];
    input.extend(ping(9).as_bytes());
    let answers = talk_bytes(&env, &input);
    assert_eq!(answers.len(), 2, "{answers:?}");
    assert_eq!(answers[0]["error"]["code"], -32700, "{}", answers[0]);
    assert_eq!(answers[0]["id"], Value::Null);
    assert_eq!(answers[1]["id"], 9);
}

#[test]
fn a_line_ending_with_a_carriage_return_is_still_a_request() {
    let env = Env::with_project();
    let input = format!("{}\r\n", json!({"jsonrpc": "2.0", "id": 4, "method": "ping"}));
    let answers = talk_bytes(&env, input.as_bytes());
    assert_eq!(answers.len(), 1, "{answers:?}");
    assert_eq!(answers[0]["id"], 4);
}

#[test]
fn a_message_that_is_not_a_json_rpc_request_gets_an_invalid_request_error() {
    let env = Env::with_project();
    let lines = [
        // Wrong protocol version.
        r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#,
        // An id that is neither a string, a number nor null.
        r#"{"jsonrpc":"2.0","id":[],"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":true,"method":"ping"}"#,
        // No method at all, and no id.
        r#"{}"#,
        // A method that is not a string.
        r#"{"jsonrpc":"2.0","id":5,"method":7}"#,
        // Parameters that are neither an object nor an array.
        r#"{"jsonrpc":"2.0","id":6,"method":"ping","params":"nope"}"#,
        // The server still answers a good request afterwards.
        r#"{"jsonrpc":"2.0","id":"ok","method":"ping"}"#,
    ];
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let answers = talk_bytes(&env, input.as_bytes());
    assert_eq!(answers.len(), lines.len(), "{answers:?}");
    for answer in &answers[..6] {
        assert_eq!(answer["error"]["code"], -32600, "{answer}");
    }
    // An id that cannot be told is answered with null; one that can is echoed.
    assert_eq!(answers[0]["id"], 1);
    assert_eq!(answers[1]["id"], Value::Null);
    assert_eq!(answers[2]["id"], Value::Null);
    assert_eq!(answers[3]["id"], Value::Null);
    assert_eq!(answers[4]["id"], 5);
    assert_eq!(answers[5]["id"], 6);
    assert_eq!(answers[6]["id"], "ok");
    assert_eq!(answers[6]["result"], json!({}));
}

#[test]
fn an_answer_or_a_notification_from_the_client_gets_no_reply() {
    let env = Env::with_project();
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
    ]
    .iter()
    .map(|l| format!("{l}\n"))
    .collect::<String>();
    let answers = talk_bytes(&env, input.as_bytes());
    assert_eq!(answers.len(), 1, "{answers:?}");
    assert_eq!(answers[0]["id"], 2);
}
