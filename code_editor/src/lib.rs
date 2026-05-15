pub mod ast;
pub mod edit;
pub mod error;

pub use ast::{ClojureAst, Form, FormKind};
pub use edit::{Edit, EditResult, Editor};
pub use error::EditorError;

pub type Result<T> = std::result::Result<T, EditorError>;
