use crate::Result;
use serde::{Deserialize, Serialize};

/// A single structural change to apply to source text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Edit {
    /// Replace the span of a named `defn` body entirely.
    ReplaceBody { fn_name: String, new_body: String },
    /// Append a form after the named `defn`'s argument vector.
    PrependToBody { fn_name: String, form: String },
    /// Wrap the body of a `defn` with an outer form (e.g. `(do ... <body>)`).
    WrapBody { fn_name: String, wrapper_prefix: String, wrapper_suffix: String },
    /// Rename a `defn`.
    Rename { old_name: String, new_name: String },
    /// Insert a new top-level form after the named form (or at end if None).
    InsertAfter { anchor: Option<String>, form: String },
}

/// The outcome of applying one or more edits.
#[derive(Debug, Clone)]
pub struct EditResult {
    pub source: String,
    pub edits_applied: usize,
}

/// Applies structural edits to Clojure source.
pub struct Editor {
    source: String,
}

impl Editor {
    pub fn new(source: impl Into<String>) -> Self {
        Self { source: source.into() }
    }

    /// Apply a sequence of edits in order, rebuilding the source after each.
    ///
    /// Edits are applied as text operations against span information from the
    /// AST. Because spans shift after each mutation, we re-parse between edits.
    pub fn apply(&self, edits: &[Edit]) -> Result<EditResult> {
        let mut source = self.source.clone();
        let mut applied = 0;

        for edit in edits {
            let result = apply_single(&source, edit)?;
            source = result;
            applied += 1;
        }

        Ok(EditResult { source, edits_applied: applied })
    }
}

fn apply_single(source: &str, edit: &Edit) -> Result<String> {
    // Placeholder: real implementation will parse the AST, locate spans,
    // and splice the new text. Returns source unchanged for now.
    tracing::debug!(?edit, "applying edit (stub)");
    Ok(source.to_owned())
}
