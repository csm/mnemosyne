//! Phase 0 of `doc/E2E-TESTING.md`: a plumbing smoke test with no LLM in the
//! loop. Every other test in the workspace drives the protocol in-process
//! via `McpServer::handle_line` (see `mnemosyne-mcp/tests/server_flow.rs`);
//! this one spawns the actual `mnemosyne-mcp-server` binary as a subprocess
//! and speaks line-delimited JSON-RPC over its real stdin/stdout pipes —
//! the process boundary and stdio framing that in-process tests can't touch.
//!
//! Covers: the `initialize` handshake, `tools/list`, one call per tool, one
//! deliberately malformed call, and one large (>10 MB) eval output surviving
//! the line-based stdout framing.

use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

const RECV_TIMEOUT: Duration = Duration::from_secs(30);

struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Server {
    async fn spawn(data_dir: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mnemosyne-mcp-server"))
            .arg("--data-dir")
            .arg(data_dir)
            .arg("--minimal-runtime")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn mnemosyne-mcp-server");
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
        Self {
            child,
            stdin,
            stdout,
        }
    }

    async fn send(&mut self, msg: Value) {
        let mut line = msg.to_string();
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .expect("write to server stdin");
        self.stdin.flush().await.expect("flush server stdin");
    }

    async fn recv(&mut self) -> Value {
        let mut line = String::new();
        let n = timeout(RECV_TIMEOUT, self.stdout.read_line(&mut line))
            .await
            .expect("server response timed out")
            .expect("read server stdout");
        assert!(n > 0, "server closed stdout without replying");
        serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("non-JSON line from server: {e}\nline: {line}"))
    }

    async fn call(&mut self, id: i64, name: &str, arguments: Value) -> Value {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        }))
        .await;
        self.recv().await
    }

    /// Close stdin (the transport's shutdown signal, per `run_stdio`) and
    /// wait for the process to exit cleanly.
    async fn shutdown(mut self) {
        drop(self.stdin);
        let status = timeout(Duration::from_secs(10), self.child.wait())
            .await
            .expect("server did not exit after stdin closed")
            .expect("wait on child process");
        assert!(status.success(), "server exited with {status}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn plumbing_smoke_test() {
    let dir = TempDir::new().unwrap();
    let mut server = Server::spawn(dir.path()).await;

    // --- initialize handshake ---
    server
        .send(json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "phase0-smoke", "version": "0" }
            }
        }))
        .await;
    let init = server.recv().await;
    assert_eq!(init["result"]["serverInfo"]["name"], "mnemosyne");
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    let instructions = init["result"]["instructions"]
        .as_str()
        .expect("instructions present");
    assert!(instructions.contains("mnemosyne.core"), "{instructions}");

    // The `initialized` notification gets no reply; nothing to read here.
    server
        .send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;

    // --- tools/list ---
    server
        .send(json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .await;
    let list = server.recv().await;
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .expect("tools array")
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

    // --- one call per tool ---
    let resp = server
        .call(2, "clojure_eval", json!({ "code": "(+ 1 2)" }))
        .await;
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    assert_eq!(resp["result"]["content"][0]["text"], "3");

    let resp = server
        .call(
            3,
            "function_lookup",
            json!({ "query": "mnemosyne.core/deep-merge", "mode": "exact" }),
        )
        .await;
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    assert!(resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("(defn deep-merge"));

    let resp = server
        .call(
            4,
            "save_function",
            json!({
                "namespace": "smoke.util",
                "name": "add",
                "source": "(defn add\n  \"Adds two numbers.\"\n  [a b]\n  (+ a b))"
            }),
        )
        .await;
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    assert!(resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("smoke.util/add@"));

    let resp = server
        .call(
            5,
            "annotate_function",
            json!({
                "function": "smoke.util/add",
                "description": "Sums two numbers.",
                "use_cases": ["smoke test"]
            }),
        )
        .await;
    assert_eq!(resp["result"]["isError"], false, "{resp}");

    // --- one deliberately malformed call ---
    // Wrong type for a required field: caught by argument deserialization,
    // surfaced as a tool-level error (`isError: true`), not a protocol error.
    let resp = server.call(6, "clojure_eval", json!({ "code": 42 })).await;
    assert!(
        resp.get("error").is_none(),
        "expected a tool error, not a JSON-RPC protocol error: {resp}"
    );
    assert_eq!(resp["result"]["isError"], true);
    assert!(resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("invalid arguments"));

    // --- one >10 MB eval output ---
    // Doubling avoids the O(n^2) blowup of `(apply str (repeat n "x"))`.
    let resp = server
        .call(
            7,
            "clojure_eval",
            json!({ "code": r#"(loop [s "x"] (if (< (count s) 10000000) (recur (str s s)) s))"# }),
        )
        .await;
    assert_eq!(resp["result"]["isError"], false, "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.len() > 10_000_000,
        "expected a >10 MB payload, got {} bytes",
        text.len()
    );
    assert!(text.trim_matches('"').chars().all(|c| c == 'x'));

    server.shutdown().await;
}
