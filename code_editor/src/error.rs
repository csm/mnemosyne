use thiserror::Error;

#[derive(Debug, Error)]
pub enum EditorError {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("form not found: {0}")]
    NotFound(String),
    #[error("invalid edit: {0}")]
    InvalidEdit(String),
    #[error("execution error: {0}")]
    Execution(#[from] execution_engine::ExecutionError),
}
