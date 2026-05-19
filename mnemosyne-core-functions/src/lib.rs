pub mod error;
pub mod loader;
pub mod registry;

pub use error::CoreError;
pub use loader::load_core;
pub use registry::{FunctionRegistry, FunctionTemplate};

pub type Result<T> = std::result::Result<T, CoreError>;

/// Clojure source files embedded at compile time so the binary ships self-contained.
pub mod embedded {
    /// Core utility functions (map, filter, reduce wrappers, etc.)
    pub const CORE_CLJ: &str = include_str!("clojure/core.clj");
    /// Template building blocks for generating new functions via the editor.
    pub const TEMPLATES_CLJ: &str = include_str!("clojure/templates.clj");
}
