pub mod error;
pub mod handle;
pub mod runtime;
pub mod value;

pub use error::ExecutionError;
pub use handle::{IoPolicy, RuntimeHandle};
pub use runtime::ClojureRuntime;
pub use value::ClojureValue;

pub type Result<T> = std::result::Result<T, ExecutionError>;
