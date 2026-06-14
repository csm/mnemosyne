use crate::{ClojureValue, ExecutionError, Result};
use cljrs_eval::{standard_env, Env, GlobalEnv};
use cljrs_reader::Parser;
use std::collections::HashMap;
use std::sync::Arc;

/// Owns a clojurust interpreter session.
///
/// `globals` holds the shared, immutable namespace table (clojure.core, etc.)
/// produced once by `standard_env()`. `env` is the mutable per-session frame
/// stack and current namespace; it borrows `globals` via `Arc`.
///
/// `loaded_versions` tracks every versioned ref that has been loaded into
/// this session, keyed by the namespace (or `namespace/symbol` for single-var
/// loads). This makes the runtime's provenance fully auditable.
pub struct ClojureRuntime {
    globals: Arc<GlobalEnv>,
    env: Env,
    loaded_versions: HashMap<String, String>,
}

impl ClojureRuntime {
    /// Boot a new runtime with the full standard library loaded.
    ///
    /// This calls `standard_env()` which eagerly compiles clojure.core;
    /// use `ClojureRuntime::minimal()` if you need a faster cold start.
    pub fn new() -> Self {
        let globals = standard_env();
        let env = Env::new(Arc::clone(&globals), "user");
        Self {
            globals,
            env,
            loaded_versions: HashMap::new(),
        }
    }

    /// Boot with only the minimal bootstrap environment (no clojure.test, etc.).
    pub fn minimal() -> Self {
        let globals = cljrs_eval::standard_env_minimal();
        let env = Env::new(Arc::clone(&globals), "user");
        Self {
            globals,
            env,
            loaded_versions: HashMap::new(),
        }
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

    /// Like [`eval`](Self::eval) but cooperatively async: `await` and the
    /// `clojure.core.async` channel operations yield on the LocalSet executor
    /// instead of blocking the thread. This is the path used when the runtime
    /// has the async IO/networking substrate installed, so file and network
    /// calls (which deliver over core.async channels) can actually make
    /// progress while an expression is being evaluated.
    ///
    /// Must be called from within a Tokio `current_thread` + `LocalSet`
    /// context.
    pub async fn eval_async(&mut self, source: &str) -> Result<ClojureValue> {
        let mut parser = Parser::new(source.to_string(), "<repl>".to_string());
        let forms = parser.parse_all().map_err(|e| ExecutionError::Parse {
            location: "<repl>".into(),
            message: format!("{e}"),
        })?;

        let mut last = ClojureValue::Nil;
        for form in &forms {
            let val = cljrs_async::eval_async::eval_async(form, &mut self.env)
                .await
                .map_err(|e| ExecutionError::Eval(format!("{e:?}")))?;
            last = ClojureValue::from(val);
        }
        Ok(last)
    }

    /// Switch the current namespace (creates it if it doesn't exist).
    pub fn set_namespace(&mut self, ns: &str) {
        self.env.current_ns = Arc::from(ns);
    }

    pub fn current_namespace(&self) -> &str {
        &self.env.current_ns
    }

    /// Load `source` into the runtime and record that `vref_str` is now the
    /// pinned version of this code.
    ///
    /// `vref_str` should be the canonical form returned by
    /// `VersionedRef::canonical()`, e.g. `"mnemosyne.core@a1b2c3d4"`.  The
    /// registry key used for deduplication is derived from the vref: the full
    /// vref for a symbol load, or just the namespace for a namespace load.
    pub fn load_versioned(&mut self, source: &str, vref_str: &str) -> Result<()> {
        self.load_string(source)?;
        self.loaded_versions
            .insert(vref_str.to_owned(), vref_str.to_owned());
        tracing::debug!(vref = vref_str, "loaded versioned symbol");
        Ok(())
    }

    /// Return a snapshot of every versioned ref that has been loaded into
    /// this runtime session.  Keys and values are both the canonical vref
    /// string (retained for forward-compatibility with richer metadata).
    pub fn loaded_versions(&self) -> &HashMap<String, String> {
        &self.loaded_versions
    }

    /// Expose the underlying `GlobalEnv` for registering native Rust functions
    /// via `cljrs_eval::GlobalEnv::intern`.
    pub fn globals(&self) -> &Arc<GlobalEnv> {
        &self.globals
    }

    /// Return the names of all vars interned in the current namespace.
    ///
    /// This captures user-defined `def`/`defn` bindings (not `clojure.core`
    /// refers). Returns an empty vec if the namespace has no user-defined vars.
    pub fn binding_names(&self) -> Vec<String> {
        let ns_name = &*self.env.current_ns;
        let namespaces = self.globals.namespaces.read().unwrap();
        let Some(ns_ptr) = namespaces.get(ns_name) else {
            return vec![];
        };
        let interns = ns_ptr.get().interns.lock().unwrap();
        interns.keys().map(|k| k.to_string()).collect()
    }
}

impl Default for ClojureRuntime {
    fn default() -> Self {
        Self::new()
    }
}
