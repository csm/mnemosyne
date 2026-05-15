use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("index error: {0}")]
    Tantivy(#[from] tantivy::TantivyError),
    #[error("query parse error: {0}")]
    QueryParse(#[from] tantivy::query::QueryParserError),
    #[error("directory error: {0}")]
    Directory(#[from] tantivy::directory::error::OpenDirectoryError),
    #[error("storage error: {0}")]
    Storage(#[from] code_storage::StorageError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
