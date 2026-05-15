use crate::{EditorError, Result};
use serde::{Deserialize, Serialize};

/// Position in source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// Discriminant for a Clojure syntactic form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FormKind {
    List,
    Vector,
    Map,
    Set,
    Symbol,
    Keyword,
    String,
    Number,
    Bool,
    Nil,
    Comment,
    Metadata,
}

/// A node in the Clojure concrete syntax tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Form {
    pub kind: FormKind,
    pub span: Span,
    pub text: String,
    pub children: Vec<Form>,
}

impl Form {
    /// Return the function name if this form is a `(defn name ...)`.
    pub fn defn_name(&self) -> Option<&str> {
        if self.kind != FormKind::List {
            return None;
        }
        let head = self.children.first()?;
        if head.text != "defn" && head.text != "defn-" {
            return None;
        }
        self.children.get(1).map(|f| f.text.as_str())
    }

    /// Recursively find all `defn` forms.
    pub fn find_defns(&self) -> Vec<&Form> {
        let mut out = Vec::new();
        if self.defn_name().is_some() {
            out.push(self);
        }
        for child in &self.children {
            out.extend(child.find_defns());
        }
        out
    }
}

/// Top-level parse result for a single Clojure source file.
pub struct ClojureAst {
    pub source: String,
    pub top_level: Vec<Form>,
}

impl ClojureAst {
    /// Parse Clojure source into an AST.
    ///
    /// TODO: integrate tree-sitter-clojure grammar once the grammar crate is
    /// stable; for now returns an error so callers can handle the stub.
    pub fn parse(_source: &str) -> Result<Self> {
        Err(EditorError::Parse(
            "tree-sitter-clojure grammar not yet wired — stub only".into(),
        ))
    }

    /// Find a top-level `defn` by name.
    pub fn find_defn(&self, name: &str) -> Option<&Form> {
        self.top_level
            .iter()
            .flat_map(|f| f.find_defns())
            .find(|f| f.defn_name() == Some(name))
    }
}
