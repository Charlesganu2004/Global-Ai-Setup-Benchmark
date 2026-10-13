//! The MCP server: initialize, tools/list and tools/call over standard input and output.

mod common;
use common::Env;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;

/// Sends the requests, one per line, closes standard input and returns the answers.
fn talk(env: &Env, requests: &[Value]) -> (Vec<Value>, String) {
    let input: String = requests.iter().map(|r| format!("{r}\n")).collect();
    let run = env.run_stdin(&["mcp"], &input);
    assert_eq!(run.code, 0, "{}", run.err);
    let answers = run.out.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON ({e}): {l}"))).collect();
    (answers, run.err)
}

fn text_of(answer: &Value) -> &str {
    answer["result"]["content"][0]["text"].as_str().unwrap_or_else(|| panic!("no text in {answer}"))
}

#[test]
fn initialize_list_and_one_call() {
    let env = Env::with_project();
    let (answers, _) = talk(
        &env,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "ask", "arguments": {"questions": ["def:load_config", "callers:main"]}}}),
        ],
    );
    // The notification gets no answer.
    assert_eq!(answers.len(), 3, "{answers:?}");

    assert_eq!(answers[0]["jsonrpc"], "2.0");
    assert_eq!(answers[0]["id"], 1);
    assert_eq!(answers[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(answers[0]["result"]["serverInfo"]["name"], "pgr");
    assert!(answers[0]["result"]["capabilities"]["tools"].is_object());

    let tools = answers[1]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["ask", "find", "recall", "retain", "pack", "handoff", "loadout"]);
    for tool in tools {
        assert_eq!(tool["inputSchema"]["type"], "object");
        assert!(tool["description"].as_str().unwrap().len() > 10);
    }

    assert_eq!(answers[2]["id"], 3);
    assert_eq!(answers[2]["result"]["isError"], false);
    assert_eq!(
        text_of(&answers[2]),
        "== def:load_config\nload_config app/core.py:4-6 (path)\n== callers:main\n1 caller of main:\n<module> app/cli.py @17\n"
    );
}

#[test]
fn every_tool_gives_the_text_of_its_command() {
    let env = Env::with_project();
    // What the commands print, taken while the bank is still empty.
    let expected_find = env.run(&["find", "-k", "1", "read the configuration file"]).out;
    let expected_pack = env.run(&["pack", "change load_config", "--budget", "300"]).out;
    let call = |id: u64, name: &str, arguments: Value| json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": arguments}});
    let (answers, _) = talk(
        &env,
        &[
            call(1, "find", json!({"query": "read the configuration file", "limit": 1})),
            call(2, "pack", json!({"task": "change load_config", "budget": 300})),
            call(3, "retain", json!({"text": "load_config reads the settings file.", "kind": "world", "tags": ["cfg"]})),
            call(4, "recall", json!({"query": "load_config settings", "limit": 3})),
        ],
    );
    assert_eq!(answers.len(), 4);
    assert_eq!(text_of(&answers[0]), expected_find);
    assert_eq!(text_of(&answers[1]), expected_pack);
    assert_eq!(text_of(&answers[2]), "kept n000001\n");
    assert!(text_of(&answers[3]).starts_with("- load_config reads the settings file.  ("), "{}", text_of(&answers[3]));
    for answer in &answers {
        assert_eq!(answer["result"]["isError"], false, "{answer}");
    }
}

#[test]
fn the_server_follows_edits_made_between_two_calls() {
    let env = Env::with_project();
    let mut cmd = env.command();
    cmd.arg("--root").arg(&env.proj).arg("mcp");
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut exchange = |request: Value| -> Value {
        writeln!(input, "{request}").unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON ({e}): {line}"))
    };
    let ask = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "ask", "arguments": {"questions": ["callers:slugify"]}}});

    let first = exchange(ask.clone());
    assert_eq!(text_of(&first), "== callers:slugify\n0 callers of slugify\n");
    let mut util = env.read("app/util.py");
    util.push_str("\n\ndef clean_name(text):\n    return slugify(text)\n");
    env.write("app/util.py", &util);
    let second = exchange(ask.clone());
    assert_eq!(text_of(&second), "== callers:slugify\n1 caller of slugify:\nclean_name app/util.py:11-12 @12\n");
    env.remove("app/util.py");
    let third = exchange(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "ask", "arguments": {"questions": ["def:slugify"]}}}));
    assert_eq!(text_of(&third), "== def:slugify\nslugify: not found\n");

    drop(exchange);
    drop(input);
    assert!(child.wait().unwrap().success());
}

#[test]
fn errors_use_the_codes_of_json_rpc() {
    let env = Env::with_project();
    let (answers, _) = talk(
        &env,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "no/such/method"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "bogus", "arguments": {}}}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "find", "arguments": {}}}),
            json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "ask", "arguments": {"questions": ["bogus:x"]}}}),
            json!({"jsonrpc": "2.0", "method": "no/such/notification"}),
        ],
    );
    assert_eq!(answers.len(), 5, "{answers:?}");
    assert_eq!(answers[0]["error"]["code"], -32601);
    assert_eq!(answers[0]["id"], 1);
    assert_eq!(answers[1]["result"], json!({}));
    assert_eq!(answers[2]["error"]["code"], -32602);
    assert_eq!(answers[3]["error"]["code"], -32602);
    // A question the command line would refuse is a tool error, not a protocol error.
    assert_eq!(answers[4]["result"]["isError"], true);
    assert!(text_of(&answers[4]).starts_with("pgr: unknown kind"), "{}", text_of(&answers[4]));
}

#[test]
fn bad_json_gets_a_parse_error_and_the_server_goes_on() {
    let env = Env::with_project();
    let input = "this is not json\n[1,2]\n{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\"}\n";
    let run = env.run_stdin(&["mcp"], input);
    assert_eq!(run.code, 0, "{}", run.err);
    let lines: Vec<Value> = run.out.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["error"]["code"], -32700);
    assert_eq!(lines[1]["error"]["code"], -32600);
    assert_eq!(lines[2]["id"], 9);
}

#[test]
fn a_tool_of_an_engine_that_is_off_reports_an_error_result() {
    let env = Env::with_project();
    let input = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "find", "arguments": {"query": "x"}}}).to_string() + "\n";
    let run = env.run_stdin(&["--engines", "graph", "mcp"], &input);
    let answer: Value = serde_json::from_str(run.out.lines().next().unwrap()).unwrap();
    assert_eq!(answer["result"]["isError"], true);
    assert_eq!(text_of(&answer), "pgr: the search engine is off\n");
}

#[test]
fn a_skipped_folder_answers_with_its_off_line() {
    let env = Env::with_project();
    env.write(".local-index-skip", "");
    let input = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "ask", "arguments": {"questions": ["def:main"]}}}).to_string() + "\n";
    let run = env.run_stdin(&["mcp"], &input);
    let answer: Value = serde_json::from_str(run.out.lines().next().unwrap()).unwrap();
    assert_eq!(answer["result"]["isError"], false);
    assert_eq!(text_of(&answer), "pgr: off for proj (it holds a .local-index-skip file)\n");
}
