use std::collections::HashMap;
use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identifier for a single agent session.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The payload of one logged episode.
///
/// Internally tagged so the `kind` discriminant is embedded in the JSON object,
/// which keeps the on-disk format flat and easy to read.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum EpisodeKind {
    /// A message received from the human user.
    UserMessage { content: String },
    /// The final assistant reply returned by the agent loop.
    AssistantReply { content: String },
    /// A tool call dispatched during the agent loop.
    ToolCall {
        tool: String,
        input: serde_json::Value,
    },
    /// The result of a dispatched tool call.
    ToolResult {
        tool: String,
        success: bool,
        output: String,
    },
    /// Snapshot of user-defined names in the active Clojure namespace.
    WorkingMemory { bindings: HashMap<String, String> },
    /// Free-form annotation the agent can write to its own log.
    Note { content: String },
}

/// One record in the episodic log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub seq: u64,
    pub session_id: SessionId,
    pub timestamp: DateTime<Utc>,
    #[serde(flatten)]
    pub kind: EpisodeKind,
}
