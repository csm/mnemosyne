use thiserror::Error;

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("invalid versioned reference '{0}': {1}")]
    InvalidRef(String, String),

    #[error("repository not found for '{0}': {1}")]
    RepoNotFound(String, String),

    #[error("symbol '{0}' not found at commit {1}")]
    SymbolNotFound(String, String),

    #[error("trust denied for '{0}': {1}")]
    TrustDenied(String, String),

    #[error("storage error: {0}")]
    Storage(#[from] mnemosyne_code_storage::StorageError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
