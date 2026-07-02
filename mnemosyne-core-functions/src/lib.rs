pub mod error;
pub mod loader;
pub mod registry;

pub use error::CoreError;
pub use loader::{load_core, load_shell};
pub use registry::{FunctionRegistry, FunctionTemplate};

pub type Result<T> = std::result::Result<T, CoreError>;

/// Clojure source files embedded at compile time so the binary ships self-contained.
pub mod embedded {
    /// Core utility functions (map, filter, reduce wrappers, etc.)
    pub const CORE_CLJ: &str = include_str!("clojure/core.clj");
    /// Template building blocks for generating new functions via the editor.
    pub const TEMPLATES_CLJ: &str = include_str!("clojure/templates.clj");
    /// Shell-style channel utilities (cat, ls, find, grep, …). Requires a
    /// runtime whose `IoPolicy` grants file IO — the substrate namespaces
    /// `clojure.core.async`, `clojure.rust.io.async`, and
    /// `mnemosyne.shell.native` must be loaded.
    pub const SHELL_CLJ: &str = include_str!("clojure/shell.clj");
}
