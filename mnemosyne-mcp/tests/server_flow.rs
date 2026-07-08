//! End-to-end exercise of the MCP server over its message seam: initialize,
//! save → lookup → annotate → lookup, and eval.

use mnemosyne_execution_engine::IoPolicy;
use mnemosyne_mcp::{build_server, McpConfig};
use mnemosyne_mcp_core::McpServer;
use serde_json::{json, Value};
use tempfile::TempDir;

async fn test_server(dir: &TempDir) -> McpServer {
    build_server(McpConfig {
        data_dir: dir.path().to_owned(),
        io_policy: IoPolicy::deny_all(),
        minimal_runtime: true,
    })
    .await
    .expect("server init")
}

async fn call_tool(server: &McpServer, name: &str, arguments: Value) -> (bool, String) {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments }
    });
    let reply = server
        .handle_line(&msg.to_string())
        .await
        .expect("tool call reply");
    let v: Value = serde_json::from_str(&reply).unwrap();
    assert!(
        v.get("error").is_none(),
        "unexpected protocol error: {reply}"
    );
    let is_error = v["result"]["isError"].as_bool().unwrap();
    let text = v["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    (is_error, text)
}

#[tokio::test(flavor = "multi_thread")]
async fn initialize_lists_all_four_tools() {
    let dir = TempDir::new().unwrap();
    let server = test_server(&dir).await;

    let init = server
        .handle_line(
            r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        )
        .await
        .unwrap();
    let init: Value = serde_json::from_str(&init).unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "mnemosyne");

    // The instructions orient a fresh agent and track this configuration:
    // minimal runtime, file IO denied.
    let instructions = init["result"]["instructions"].as_str().unwrap();
    assert!(instructions.contains("mnemosyne.core"), "{instructions}");
    assert!(instructions.contains("mnemosyne.shell"), "{instructions}");
    assert!(instructions.contains("NOT preloaded"), "{instructions}");
    assert!(instructions.contains("--allow-file-io"), "{instructions}");

    let list = server
        .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
        .await
        .unwrap();
    let list: Value = serde_json::from_str(&list).unwrap();
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec![
            "clojure_eval",
            "function_lookup",
            "save_function",
            "annotate_function"
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn builtins_are_discoverable_on_a_fresh_store() {
    let dir = TempDir::new().unwrap();
    let server = test_server(&dir).await;

    // Exact lookup of a built-in works before anything has been saved.
    let (err, text) = call_tool(
        &server,
        "function_lookup",
        json!({ "query": "mnemosyne.core/deep-merge", "mode": "exact" }),
    )
    .await;
    assert!(!err, "{text}");
    assert!(text.contains("(defn deep-merge"), "{text}");
    assert!(text.contains("mnemosyne.core/deep-merge@"), "{text}");

    // Full-text search over built-in docstrings works from the first query.
    let (err, text) = call_tool(
        &server,
        "function_lookup",
        json!({ "query": "recursively merge maps", "mode": "fulltext" }),
    )
    .await;
    assert!(!err, "{text}");
    assert!(text.contains("mnemosyne.core/deep-merge"), "{text}");

    // The shell namespace is indexed too, even though this session's IO
    // policy prevents loading it into the runtime.
    let (err, text) = call_tool(
        &server,
        "function_lookup",
        json!({ "query": "mnemosyne.shell/grep", "mode": "exact" }),
    )
    .await;
    assert!(!err, "{text}");
    assert!(text.contains("(defn grep"), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn eval_returns_value_and_persists_definitions() {
    let dir = TempDir::new().unwrap();
    let server = test_server(&dir).await;

    let (err, text) = call_tool(&server, "clojure_eval", json!({ "code": "(+ 1 2)" })).await;
    assert!(!err, "{text}");
    assert_eq!(text, "3");

    // Definitions persist across calls in the same session.
    let (err, _) = call_tool(
        &server,
        "clojure_eval",
        json!({ "code": "(def scratch-x 21)" }),
    )
    .await;
    assert!(!err);
    let (err, text) = call_tool(
        &server,
        "clojure_eval",
        json!({ "code": "(* scratch-x 2)" }),
    )
    .await;
    assert!(!err, "{text}");
    assert_eq!(text, "42");

    // Errors surface as tool errors, not protocol errors.
    let (err, text) = call_tool(&server, "clojure_eval", json!({ "code": "(unbalanced" })).await;
    assert!(err);
    assert!(text.contains("error"), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn save_lookup_annotate_round_trip() {
    let dir = TempDir::new().unwrap();
    let server = test_server(&dir).await;

    // The store is never empty (it is seeded with the built-in library), so
    // looking up a namespace that doesn't exist is a plain not-found error.
    let (err, text) = call_tool(
        &server,
        "function_lookup",
        json!({ "query": "scratch.util/add", "mode": "exact" }),
    )
    .await;
    assert!(err);
    assert!(text.contains("not found"), "{text}");

    // Save a function.
    let (err, text) = call_tool(
        &server,
        "save_function",
        json!({
            "namespace": "scratch.util",
            "name": "add",
            "source": "(defn add\n  \"Adds two numbers.\"\n  [a b]\n  (+ a b))"
        }),
    )
    .await;
    assert!(!err, "{text}");
    assert!(text.contains("scratch.util/add@"), "{text}");

    // Exact lookup returns the source with provenance.
    let (err, text) = call_tool(
        &server,
        "function_lookup",
        json!({ "query": "scratch.util/add" }),
    )
    .await;
    assert!(!err, "{text}");
    assert!(text.contains("(defn add"), "{text}");
    assert!(text.contains("trust:"), "{text}");

    // Full-text search finds it by docstring words.
    let (err, text) = call_tool(
        &server,
        "function_lookup",
        json!({ "query": "adds numbers", "mode": "fulltext" }),
    )
    .await;
    assert!(!err, "{text}");
    assert!(text.contains("scratch.util/add"), "{text}");

    // Annotate it.
    let (err, text) = call_tool(
        &server,
        "annotate_function",
        json!({
            "function": "scratch.util/add",
            "description": "Sums two numbers.",
            "use_cases": ["arithmetic", "aggregating counters"]
        }),
    )
    .await;
    assert!(!err, "{text}");
    assert!(text.contains("Annotated scratch.util/add"), "{text}");

    // The annotation shows up in exact lookup…
    let (err, text) = call_tool(
        &server,
        "function_lookup",
        json!({ "query": "scratch.util/add", "mode": "exact" }),
    )
    .await;
    assert!(!err, "{text}");
    assert!(text.contains("Sums two numbers."), "{text}");
    assert!(text.contains("aggregating counters"), "{text}");

    // …and makes the function findable by annotation text.
    let (err, text) = call_tool(
        &server,
        "function_lookup",
        json!({ "query": "aggregating counters", "mode": "fulltext" }),
    )
    .await;
    assert!(!err, "{text}");
    assert!(text.contains("scratch.util/add"), "{text}");

    // Saving again replaces the definition in place.
    let (err, text) = call_tool(
        &server,
        "save_function",
        json!({
            "namespace": "scratch.util",
            "name": "add",
            "source": "(defn add\n  \"Adds two numbers, v2.\"\n  [a b]\n  (+ a b 0))"
        }),
    )
    .await;
    assert!(!err, "{text}");

    let (err, text) = call_tool(
        &server,
        "function_lookup",
        json!({ "query": "scratch.util/add" }),
    )
    .await;
    assert!(!err, "{text}");
    assert!(text.contains("v2"), "{text}");
    assert!(
        !text.contains("(+ a b))"),
        "old body should be replaced: {text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn annotate_unknown_function_is_rejected() {
    let dir = TempDir::new().unwrap();
    let server = test_server(&dir).await;

    let (err, text) = call_tool(
        &server,
        "annotate_function",
        json!({ "function": "no.such/fn", "description": "nope" }),
    )
    .await;
    assert!(err);
    assert!(text.contains("save_function"), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn save_rejects_mismatched_name() {
    let dir = TempDir::new().unwrap();
    let server = test_server(&dir).await;

    let (err, text) = call_tool(
        &server,
        "save_function",
        json!({
            "namespace": "scratch.util",
            "name": "mul",
            "source": "(defn other [x] x)"
        }),
    )
    .await;
    assert!(err);
    assert!(text.contains("mul"), "{text}");
    assert!(text.contains("other"), "{text}");
}
