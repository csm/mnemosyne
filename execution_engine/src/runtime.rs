use crate::{ClojureValue, ExecutionError, Result};
use std::collections::HashMap;

/// Handle to the Clojure interpreter.
///
/// Backed by clojure-rs (or a compatible runtime); this struct owns the
/// interpreter state and serializes access so callers don't need to.
pub struct ClojureRuntime {
    // Namespace → var → value; placeholder until a real runtime is wired in.
    namespaces: HashMap<String, HashMap<String, ClojureValue>>,
    current_ns: String,
}

impl ClojureRuntime {
    pub fn new() -> Self {
        Self {
            namespaces: HashMap::from([("user".into(), HashMap::new())]),
            current_ns: "user".into(),
        }
    }

    /// Evaluate a Clojure expression string and return the result.
    ///
    /// TODO: delegate to clojure-rs once the crate dependency is pinned.
    pub fn eval(&mut self, source: &str) -> Result<ClojureValue> {
        tracing::debug!(ns = %self.current_ns, source, "eval");
        Err(ExecutionError::Eval(
            "runtime not yet wired to clojure-rs — stub only".into(),
        ))
    }

    /// Load a full namespace from source text.
    pub fn load_string(&mut self, source: &str) -> Result<()> {
        self.eval(source)?;
        Ok(())
    }

    /// Switch the current namespace.
    pub fn set_namespace(&mut self, ns: &str) {
        self.current_ns = ns.to_owned();
        self.namespaces.entry(ns.to_owned()).or_default();
    }

    pub fn current_namespace(&self) -> &str {
        &self.current_ns
    }
}

impl Default for ClojureRuntime {
    fn default() -> Self {
        Self::new()
    }
}
