pub mod error;
pub mod index;
pub mod query;

pub use error::SearchError;
pub use index::{CodeIndex, IndexedFunction};
pub use query::{SearchQuery, SearchResult};

pub type Result<T> = std::result::Result<T, SearchError>;
