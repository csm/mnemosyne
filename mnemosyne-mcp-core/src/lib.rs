//! Model Context Protocol (MCP) core for Mnemosyne.
//!
//! A deliberately small, self-contained implementation of the server side of
//! MCP: JSON-RPC 2.0 framing, the lifecycle handshake (`initialize`),
//! `tools/list` / `tools/call` dispatch, and a newline-delimited-JSON stdio
//! transport. It carries no Mnemosyne dependencies so any project can embed
//! an MCP server by implementing [`McpTool`] and registering it on an
//! [`McpServer`].
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use mnemosyne_mcp_core::{CallToolResult, McpServer, McpTool};
//!
//! struct Echo;
//!
//! #[async_trait::async_trait]
//! impl McpTool for Echo {
//!     fn name(&self) -> &str { "echo" }
//!     fn description(&self) -> &str { "Echo the input back" }
//!     fn input_schema(&self) -> serde_json::Value {
//!         serde_json::json!({
//!             "type": "object",
//!             "properties": { "text": { "type": "string" } },
//!             "required": ["text"]
//!         })
//!     }
//!     async fn call(&self, args: serde_json::Value) -> CallToolResult {
//!         CallToolResult::text(args["text"].as_str().unwrap_or("").to_owned())
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     McpServer::new("echo-server", "0.1.0")
//!         .tool(Arc::new(Echo))
//!         .run_stdio()
//!         .await
//! }
//! ```

pub mod jsonrpc;
pub mod server;
pub mod tool;
pub mod types;

pub use jsonrpc::{ErrorObject, Request, Response};
pub use server::McpServer;
pub use tool::McpTool;
pub use types::{
    CallToolResult, Content, InitializeResult, ServerCapabilities, ServerInfo, ToolDescriptor,
    LATEST_PROTOCOL_VERSION, SUPPORTED_PROTOCOL_VERSIONS,
};
