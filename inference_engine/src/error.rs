use thiserror::Error;

#[derive(Debug, Error)]
pub enum InferenceError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("execution error: {0}")]
    Execution(#[from] execution_engine::ExecutionError),
    #[error("search error: {0}")]
    Search(#[from] code_search::SearchError),
    #[error("editor error: {0}")]
    Editor(#[from] code_editor::EditorError),
    #[error("storage error: {0}")]
    Storage(#[from] code_storage::StorageError),
    #[error("llm error: {status} — {message}")]
    Llm { status: u16, message: String },
    #[error("unknown tool: {0}")]
    UnknownTool(String),
}
