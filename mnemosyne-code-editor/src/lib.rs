pub mod ast;
pub mod edit;
pub mod error;
pub mod zipper;

pub use ast::{ClojureAst, Form, FormKind, Span};
pub use edit::{edit_description, Edit, EditResult, Editor};
pub use error::EditorError;
pub use zipper::{unparse, Zipper};

pub type Result<T> = std::result::Result<T, EditorError>;
