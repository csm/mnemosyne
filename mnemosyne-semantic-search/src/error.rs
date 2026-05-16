use thiserror::Error;

#[derive(Debug, Error)]
pub enum SemanticSearchError {
    #[error("embedder error: {0}")]
    Embed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("index is empty")]
    EmptyIndex,
}
