use crate::{ClojureValue, ExecutionError, Result};
use cljrs_eval::{standard_env, Env, GlobalEnv};
use cljrs_reader::Parser;
use std::sync::Arc;

/// Owns a clojurust interpreter session.
///
/// `globals` holds the shared, immutable namespace table (clojure.core, etc.)
/// produced once by `standard_env()`. `env` is the mutable per-session frame
/// stack and current namespace; it borrows `globals` via `Arc`.
pub struct ClojureRuntime {
    globals: Arc<GlobalEnv>,
    env: Env,
}

impl ClojureRuntime {
    /// Boot a new runtime with the full standard library loaded.
    ///
    /// This calls `standard_env()` which eagerly compiles clojure.core;
    /// use `ClojureRuntime::minimal()` if you need a faster cold start.
    pub fn new() -> Self {
        let globals = standard_env();
        let env = Env::new(Arc::clone(&globals), "user");
        Self { globals, env }
    }

    /// Boot with only the minimal bootstrap environment (no clojure.test, etc.).
    pub fn minimal() -> Self {
        let globals = cljrs_eval::standard_env_minimal();
        let env = Env::new(Arc::clone(&globals), "user");
        Self { globals, env }
    }

    /// Parse and evaluate all forms in `source`, returning the value of the last form.
    pub fn eval(&mut self, source: &str) -> Result<ClojureValue> {
        tracing::debug!(ns = %self.env.current_ns, "eval");
        let mut parser = Parser::new(source.to_string(), "<repl>".to_string());
        let forms = parser.parse_all().map_err(|e| ExecutionError::Parse {
            location: "<repl>".into(),
            message: format!("{e}"),
        })?;

        let mut last = ClojureValue::Nil;
        for form in &forms {
            let val = self
                .env
                .eval(form)
                .map_err(|e| ExecutionError::Eval(format!("{e:?}")))?;
            last = ClojureValue::from(val);
        }
        Ok(last)
    }

    /// Load a Clojure source string into the current namespace.
    pub fn load_string(&mut self, source: &str) -> Result<()> {
        self.eval(source).map(|_| ())
    }

    /// Switch the current namespace (creates it if it doesn't exist).
    pub fn set_namespace(&mut self, ns: &str) {
        self.env.current_ns = Arc::from(ns);
    }

    pub fn current_namespace(&self) -> &str {
        &self.env.current_ns
    }

    /// Expose the underlying `GlobalEnv` for registering native Rust functions
    /// via `cljrs_eval::GlobalEnv::intern`.
    pub fn globals(&self) -> &Arc<GlobalEnv> {
        &self.globals
    }
}

impl Default for ClojureRuntime {
    fn default() -> Self {
        Self::new()
    }
}
