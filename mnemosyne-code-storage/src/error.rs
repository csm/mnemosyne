use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("git error: {0}")]
    Git(#[from] git2::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("repository not found: {0}")]
    NotFound(String),
    #[error("invalid ref: {0}")]
    InvalidRef(String),
}
