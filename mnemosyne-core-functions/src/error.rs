use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("execution error: {0}")]
    Execution(#[from] mnemosyne_execution_engine::ExecutionError),
    #[error("function not found: {0}")]
    NotFound(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
