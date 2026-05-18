pub mod error;
pub mod repo;

pub use error::StorageError;
pub use repo::{CodeRepository, CommitAuthor, FileEntry, RepoSource};

pub type Result<T> = std::result::Result<T, StorageError>;
