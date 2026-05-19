use cljrs_reader::FormKind as RK;
use serde::{Deserialize, Serialize};

use crate::{EditorError, Result};

/// Position in source text (half-open byte range).
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
    /// The original source text for this form (preserves whitespace / comments).
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
        // Skip over metadata wrappers on the name symbol.
        let name_child = self.children.get(1)?;
        if name_child.kind == FormKind::Metadata {
            name_child.children.get(1).map(|f| f.text.as_str())
        } else {
            Some(name_child.text.as_str())
        }
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
    /// Parse Clojure source into an AST using cljrs-reader.
    pub fn parse(source: &str) -> Result<Self> {
        let mut parser = cljrs_reader::Parser::new(source.to_owned(), "<editor>".into());
        let reader_forms = parser
            .parse_all()
            .map_err(|e| EditorError::Parse(e.to_string()))?;
        let top_level = reader_forms
            .iter()
            .map(|f| from_reader_form(f, source))
            .collect();
        Ok(ClojureAst {
            source: source.to_owned(),
            top_level,
        })
    }

    /// Find a top-level `defn` by name.
    pub fn find_defn(&self, name: &str) -> Option<&Form> {
        self.top_level
            .iter()
            .flat_map(|f| f.find_defns())
            .find(|f| f.defn_name() == Some(name))
    }
}

// ── Conversion from cljrs-reader Form ────────────────────────────────────────

fn from_reader_form(f: &cljrs_reader::Form, source: &str) -> Form {
    let span = Span {
        start: f.span.start,
        end: f.span.end,
    };
    let text = source[span.start..span.end].to_owned();

    let (kind, children) = match &f.kind {
        RK::List(cs) => (FormKind::List, map_children(cs, source)),
        RK::Vector(cs) => (FormKind::Vector, map_children(cs, source)),
        RK::Map(cs) => (FormKind::Map, map_children(cs, source)),
        RK::Set(cs) => (FormKind::Set, map_children(cs, source)),
        RK::AnonFn(cs) => (FormKind::List, map_children(cs, source)),
        RK::ReaderCond { clauses, .. } => (FormKind::List, map_children(clauses, source)),

        RK::Symbol(_) => (FormKind::Symbol, vec![]),
        RK::Keyword(_) | RK::AutoKeyword(_) => (FormKind::Keyword, vec![]),
        RK::Str(_) => (FormKind::String, vec![]),
        RK::Bool(_) => (FormKind::Bool, vec![]),
        RK::Nil => (FormKind::Nil, vec![]),

        // Reader macros that wrap a single form — represent as List so
        // find_defns can still recurse into them.
        RK::Quote(inner)
        | RK::SyntaxQuote(inner)
        | RK::Unquote(inner)
        | RK::UnquoteSplice(inner)
        | RK::Deref(inner)
        | RK::Var(inner) => (FormKind::List, vec![from_reader_form(inner, source)]),

        // ^meta annotated-form — two children: [meta, form]
        RK::Meta(meta, annotated) => (
            FormKind::Metadata,
            vec![
                from_reader_form(meta, source),
                from_reader_form(annotated, source),
            ],
        ),

        RK::TaggedLiteral(_, inner) => (FormKind::List, vec![from_reader_form(inner, source)]),

        // Numeric and other leaf atoms
        _ => (FormKind::Number, vec![]),
    };

    Form {
        kind,
        span,
        text,
        children,
    }
}

fn map_children(cs: &[cljrs_reader::Form], source: &str) -> Vec<Form> {
    cs.iter().map(|c| from_reader_form(c, source)).collect()
}
