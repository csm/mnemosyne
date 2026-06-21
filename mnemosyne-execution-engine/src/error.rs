use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("parse error at {location}: {message}")]
    Parse { location: String, message: String },
    #[error("eval error: {0}")]
    Eval(String),
    #[error("runtime not initialized")]
    Uninitialized,
    #[error("timeout")]
    Timeout,
    #[error("clojure runtime thread is no longer running")]
    RuntimeGone,
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}
