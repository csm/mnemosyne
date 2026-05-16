pub mod embedder;
pub mod error;
pub mod index;

pub use embedder::{EmbedModel, Embedder, DIMENSION};
pub use error::SemanticSearchError;
pub use index::{SemanticIndex, SemanticResult};

pub type Result<T> = std::result::Result<T, SemanticSearchError>;
