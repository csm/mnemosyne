//! MCP request routing and the stdio transport.

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::jsonrpc::{
    Request, Response, INTERNAL_ERROR, INVALID_PARAMS, METHOD_NOT_FOUND, PARSE_ERROR,
};
use crate::tool::McpTool;
use crate::types::{
    InitializeResult, ServerCapabilities, ServerInfo, ToolDescriptor, LATEST_PROTOCOL_VERSION,
    SUPPORTED_PROTOCOL_VERSIONS,
};

/// An MCP server: identity, optional usage instructions, and a set of tools.
///
/// The server is transport-agnostic at its core — [`McpServer::handle_line`]
/// maps one inbound JSON-RPC message to at most one outbound message — with
/// [`McpServer::run_stdio`] providing the standard newline-delimited stdio
/// transport on top.
pub struct McpServer {
    info: ServerInfo,
    instructions: Option<String>,
    /// Registration order is preserved for `tools/list`.
    tools: Vec<Arc<dyn McpTool>>,
}

impl McpServer {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            info: ServerInfo {
                name: name.into(),
                version: version.into(),
            },
            instructions: None,
            tools: Vec::new(),
        }
    }

    /// Set the `instructions` string returned from `initialize` (guidance the
    /// client injects into the model's context).
    pub fn instructions(mut self, text: impl Into<String>) -> Self {
        self.instructions = Some(text.into());
        self
    }

    /// Register a tool (builder style).
    pub fn tool(mut self, tool: Arc<dyn McpTool>) -> Self {
        self.add_tool(tool);
        self
    }

    pub fn add_tool(&mut self, tool: Arc<dyn McpTool>) {
        self.tools.push(tool);
    }

    /// Handle one raw inbound message; returns the serialized response, or
    /// `None` when no reply is due (notifications, client responses, blanks).
    pub async fn handle_line(&self, line: &str) -> Option<String> {
        let raw: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                let resp = Response::error(Value::Null, PARSE_ERROR, format!("parse error: {e}"));
                return serde_json::to_string(&resp).ok();
            }
        };

        // A message without `method` is a response to a server-initiated
        // request; we never send any, so ignore it.
        if raw.get("method").is_none() {
            return None;
        }

        let req: Request = match serde_json::from_value(raw) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::error(
                    Value::Null,
                    PARSE_ERROR,
                    format!("malformed request: {e}"),
                );
                return serde_json::to_string(&resp).ok();
            }
        };

        let resp = self.handle_request(req).await?;
        match serde_json::to_string(&resp) {
            Ok(s) => Some(s),
            Err(e) => {
                let fallback =
                    Response::error(resp.id, INTERNAL_ERROR, format!("serialization error: {e}"));
                serde_json::to_string(&fallback).ok()
            }
        }
    }

    async fn handle_request(&self, req: Request) -> Option<Response> {
        tracing::debug!(method = %req.method, notification = req.is_notification(), "mcp message");

        if req.is_notification() {
            // `notifications/initialized`, `notifications/cancelled`, … —
            // nothing to do for a stateless tools-only server.
            return None;
        }
        let id = req.id.clone().unwrap_or(Value::Null);

        let resp = match req.method.as_str() {
            "initialize" => Response::success(id, self.initialize(req.params.as_ref())),
            "ping" => Response::success(id, json!({})),
            "tools/list" => Response::success(id, self.list_tools()),
            "tools/call" => self.call_tool(id, req.params).await,
            other => Response::error(id, METHOD_NOT_FOUND, format!("method not found: {other}")),
        };
        Some(resp)
    }

    fn initialize(&self, params: Option<&Value>) -> Value {
        // Echo the client's protocol version when we support it; otherwise
        // offer the newest revision we implement.
        let requested = params
            .and_then(|p| p.get("protocolVersion"))
            .and_then(|v| v.as_str());
        let version = match requested {
            Some(v) if SUPPORTED_PROTOCOL_VERSIONS.contains(&v) => v,
            _ => LATEST_PROTOCOL_VERSION,
        };

        let result = InitializeResult {
            protocol_version: version.to_owned(),
            capabilities: ServerCapabilities::default(),
            server_info: self.info.clone(),
            instructions: self.instructions.clone(),
        };
        serde_json::to_value(result).unwrap_or(Value::Null)
    }

    fn list_tools(&self) -> Value {
        let tools: Vec<ToolDescriptor> = self
            .tools
            .iter()
            .map(|t| ToolDescriptor {
                name: t.name().to_owned(),
                description: t.description().to_owned(),
                input_schema: t.input_schema(),
            })
            .collect();
        json!({ "tools": tools })
    }

    async fn call_tool(&self, id: Value, params: Option<Value>) -> Response {
        let Some(params) = params else {
            return Response::error(id, INVALID_PARAMS, "missing params");
        };
        let Some(name) = params.get("name").and_then(|v| v.as_str()) else {
            return Response::error(id, INVALID_PARAMS, "missing tool name");
        };
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let Some(tool) = self.tools.iter().find(|t| t.name() == name) else {
            return Response::error(id, INVALID_PARAMS, format!("unknown tool: {name}"));
        };

        tracing::info!(tool = name, "tools/call");
        let result = tool.call(arguments).await;
        match serde_json::to_value(&result) {
            Ok(v) => Response::success(id, v),
            Err(e) => Response::error(id, INTERNAL_ERROR, format!("serialization error: {e}")),
        }
    }

    /// Serve MCP over stdio: one JSON-RPC message per line on stdin, one per
    /// line on stdout. Anything the process wants to log must go to stderr.
    pub async fn run_stdio(&self) -> std::io::Result<()> {
        tracing::info!(server = %self.info.name, tools = self.tools.len(), "mcp server listening on stdio");
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        let mut stdout = tokio::io::stdout();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            if let Some(reply) = self.handle_line(&line).await {
                stdout.write_all(reply.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
        }
        tracing::info!("stdin closed; shutting down");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CallToolResult;

    struct Echo;

    #[async_trait::async_trait]
    impl McpTool for Echo {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "Echo the input back"
        }
        fn input_schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            })
        }
        async fn call(&self, arguments: Value) -> CallToolResult {
            match arguments.get("text").and_then(|v| v.as_str()) {
                Some(t) => CallToolResult::text(t.to_owned()),
                None => CallToolResult::error("missing text"),
            }
        }
    }

    fn server() -> McpServer {
        McpServer::new("test-server", "0.0.1")
            .instructions("test instructions")
            .tool(Arc::new(Echo))
    }

    async fn roundtrip(msg: &str) -> Value {
        let reply = server().handle_line(msg).await.expect("expected a reply");
        serde_json::from_str(&reply).unwrap()
    }

    #[tokio::test]
    async fn initialize_negotiates_known_version() {
        let resp = roundtrip(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}"#,
        )
        .await;
        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(resp["result"]["serverInfo"]["name"], "test-server");
        assert_eq!(resp["result"]["instructions"], "test instructions");
    }

    #[tokio::test]
    async fn initialize_falls_back_to_latest_version() {
        let resp = roundtrip(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
        )
        .await;
        assert_eq!(resp["result"]["protocolVersion"], LATEST_PROTOCOL_VERSION);
    }

    #[tokio::test]
    async fn tools_list_returns_descriptor() {
        let resp = roundtrip(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).await;
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "echo");
        assert_eq!(tools[0]["inputSchema"]["type"], "object");
    }

    #[tokio::test]
    async fn tools_call_dispatches() {
        let resp = roundtrip(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"text":"hi"}}}"#,
        )
        .await;
        assert_eq!(resp["result"]["isError"], false);
        assert_eq!(resp["result"]["content"][0]["text"], "hi");
    }

    #[tokio::test]
    async fn tool_execution_error_is_result_not_protocol_error() {
        let resp = roundtrip(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"echo","arguments":{}}}"#,
        )
        .await;
        assert!(resp.get("error").is_none());
        assert_eq!(resp["result"]["isError"], true);
    }

    #[tokio::test]
    async fn unknown_tool_is_invalid_params() {
        let resp = roundtrip(
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope"}}"#,
        )
        .await;
        assert_eq!(resp["error"]["code"], INVALID_PARAMS);
    }

    #[tokio::test]
    async fn unknown_method_is_method_not_found() {
        let resp = roundtrip(r#"{"jsonrpc":"2.0","id":6,"method":"resources/list"}"#).await;
        assert_eq!(resp["error"]["code"], METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn notifications_get_no_reply() {
        let out = server()
            .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .await;
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn client_responses_are_ignored() {
        let out = server()
            .handle_line(r#"{"jsonrpc":"2.0","id":9,"result":{}}"#)
            .await;
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn garbage_is_parse_error() {
        let resp = roundtrip("{not json").await;
        assert_eq!(resp["error"]["code"], PARSE_ERROR);
        assert_eq!(resp["id"], Value::Null);
    }
}
