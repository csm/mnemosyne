pub mod adapter;
pub mod anthropic;
pub mod error;
pub mod openai_compat;
pub mod server;

pub use adapter::{AgentResponse, ChatMessage, InterfaceAdapter};
pub use anthropic::AnthropicBackend;
pub use error::InterfaceError;
pub use openai_compat::OpenAiCompatBackend;
pub use server::{run_server, ServerConfig};
