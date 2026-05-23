//! Pluggable adapter trait matching the architecture-doc contract:
//!
//! ```text
//! trait InterfaceAdapter {
//!     async fn recv(&mut self) -> Message;
//!     async fn send(&mut self, msg: AgentResponse);
//! }
//! ```
//!
//! The HTTP/SSE server is a higher-level construct built on top of the inference
//! engine directly. This trait exists for adapters that follow the linear
//! recv → run → send loop (e.g. stdio, Slack).

use async_trait::async_trait;

use crate::error::InterfaceError;

pub struct ChatMessage {
    pub session_id: String,
    pub content: String,
}

pub struct AgentResponse {
    pub session_id: String,
    pub content: String,
}

#[async_trait]
pub trait InterfaceAdapter: Send + Sync {
    /// Block until the next user message arrives.
    async fn recv(&mut self) -> Option<ChatMessage>;
    /// Deliver the agent's response back to the caller.
    async fn send(&mut self, msg: AgentResponse) -> Result<(), InterfaceError>;
}
