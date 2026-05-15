pub mod error;
pub mod llm;
pub mod engine;
pub mod tool;

pub use error::InferenceError;
pub use llm::{LlmBackend, LlmRequest, LlmResponse, Message, Role};
pub use engine::InferenceEngine;
pub use tool::{Tool, ToolCall, ToolResult};

pub type Result<T> = std::result::Result<T, InferenceError>;
