use crate::ast::{ClojureAst, Form, FormKind};
use crate::{EditorError, Result};
use serde::{Deserialize, Serialize};

/// A single structural change to apply to source text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Edit {
    /// Replace the span of a named `defn` body entirely.
    ReplaceBody { fn_name: String, new_body: String },
    /// Insert a form at the start of a named `defn`'s body.
    PrependToBody { fn_name: String, form: String },
    /// Wrap the body of a `defn` with an outer form.
    /// The body is placed between `wrapper_prefix` and `wrapper_suffix`.
    WrapBody {
        fn_name: String,
        wrapper_prefix: String,
        wrapper_suffix: String,
    },
    /// Rename a `defn` declaration (call sites are not updated).
    Rename { old_name: String, new_name: String },
    /// Insert a new top-level form after the named `defn`, or at end of file
    /// when `anchor` is `None`.
    InsertAfter {
        anchor: Option<String>,
        form: String,
    },
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
        Self {
            source: source.into(),
        }
    }

    /// Apply a sequence of edits in order, re-parsing between each.
    pub fn apply(&self, edits: &[Edit]) -> Result<EditResult> {
        let mut source = self.source.clone();
        let mut applied = 0;
        for edit in edits {
            source = apply_single(&source, edit)?;
            applied += 1;
        }
        Ok(EditResult {
            source,
            edits_applied: applied,
        })
    }
}

// ── Public edit summary (used by the inference engine for commit messages) ───

pub fn edit_description(edit: &Edit) -> String {
    match edit {
        Edit::ReplaceBody { fn_name, .. } => format!("replace body of {fn_name}"),
        Edit::PrependToBody { fn_name, .. } => format!("prepend form to {fn_name}"),
        Edit::WrapBody { fn_name, .. } => format!("wrap body of {fn_name}"),
        Edit::Rename { old_name, new_name } => format!("rename {old_name} -> {new_name}"),
        Edit::InsertAfter { anchor: None, .. } => "insert form at end".into(),
        Edit::InsertAfter {
            anchor: Some(name), ..
        } => format!("insert after {name}"),
    }
}

// ── Core edit dispatch ────────────────────────────────────────────────────────

fn apply_single(source: &str, edit: &Edit) -> Result<String> {
    // Try the AST-driven path first; fall back to byte-scanning if parse fails.
    match apply_via_ast(source, edit) {
        Ok(result) => return Ok(result),
        Err(EditorError::Parse(_)) => {} // fall through to legacy path
        Err(e) => return Err(e),
    }
    apply_via_scan(source, edit)
}

// ── AST-driven edits ──────────────────────────────────────────────────────────

fn apply_via_ast(source: &str, edit: &Edit) -> Result<String> {
    let ast = ClojureAst::parse(source)?;

    match edit {
        Edit::ReplaceBody { fn_name, new_body } => {
            let defn = ast
                .find_defn(fn_name)
                .ok_or_else(|| EditorError::NotFound(fn_name.clone()))?;
            let (args_end, defn_end) = body_bounds(defn, fn_name)?;
            Ok(format!(
                "{}\n  {new_body}\n{}",
                &source[..args_end],
                &source[defn_end - 1..],
            ))
        }

        Edit::PrependToBody { fn_name, form } => {
            let defn = ast
                .find_defn(fn_name)
                .ok_or_else(|| EditorError::NotFound(fn_name.clone()))?;
            let (args_end, _) = body_bounds(defn, fn_name)?;
            Ok(format!(
                "{}\n  {form}{}",
                &source[..args_end],
                &source[args_end..],
            ))
        }

        Edit::WrapBody {
            fn_name,
            wrapper_prefix,
            wrapper_suffix,
        } => {
            let defn = ast
                .find_defn(fn_name)
                .ok_or_else(|| EditorError::NotFound(fn_name.clone()))?;
            let (args_end, defn_end) = body_bounds(defn, fn_name)?;
            let body = source[args_end..defn_end - 1].trim();
            Ok(format!(
                "{}\n  {wrapper_prefix}{body}{wrapper_suffix}\n{}",
                &source[..args_end],
                &source[defn_end - 1..],
            ))
        }

        Edit::Rename { old_name, new_name } => {
            let defn = ast
                .find_defn(old_name)
                .ok_or_else(|| EditorError::NotFound(old_name.clone()))?;
            let name_form = name_child(defn)
                .ok_or_else(|| EditorError::InvalidEdit(format!("no name in defn {old_name}")))?;
            Ok(format!(
                "{}{new_name}{}",
                &source[..name_form.span.start],
                &source[name_form.span.end..],
            ))
        }

        Edit::InsertAfter { anchor: None, form } => Ok(format!("{}\n\n{form}", source.trim_end())),

        Edit::InsertAfter {
            anchor: Some(name),
            form,
        } => {
            let defn = ast
                .find_defn(name)
                .ok_or_else(|| EditorError::NotFound(name.clone()))?;
            let defn_end = defn.span.end;
            Ok(format!(
                "{}\n\n{form}{}",
                &source[..defn_end],
                &source[defn_end..],
            ))
        }
    }
}

// ── AST span helpers ──────────────────────────────────────────────────────────

/// Return `(args_vec_end, defn_end)` for a defn Form.
///
/// Handles both single-arity `(defn f [x] body)` and multi-arity
/// `(defn f ([x] body) ([x y] body))`. For multi-arity, body_bounds
/// returns the end of the first arity clause's args vec and the closing
/// paren of the entire defn — callers that need per-arity work should
/// call `arity_bounds` directly.
fn body_bounds(defn: &Form, fn_name: &str) -> Result<(usize, usize)> {
    // Find the args vector: it is the first Vector child of the defn List
    // (single-arity) OR the first Vector inside the first List child that
    // follows the name symbol (multi-arity).
    let args_vec = args_vec_form(defn)
        .ok_or_else(|| EditorError::InvalidEdit(format!("no argument vector in {fn_name}")))?;
    Ok((args_vec.span.end, defn.span.end))
}

/// Find the `[args]` Vector form within a defn.
fn args_vec_form(defn: &Form) -> Option<&Form> {
    // Single-arity: direct Vector child after the name.
    for child in &defn.children {
        if child.kind == FormKind::Vector {
            return Some(child);
        }
        // Multi-arity: first List child after name/docstring contains the Vector.
        if child.kind == FormKind::List {
            for inner in &child.children {
                if inner.kind == FormKind::Vector {
                    return Some(inner);
                }
            }
        }
    }
    None
}

/// Return the name Symbol form (the identifier, not the `defn` keyword).
fn name_child(defn: &Form) -> Option<&Form> {
    // children: [defn-kw, name, ?docstring, args-or-arities...]
    // Skip metadata wrappers on the name position.
    let raw = defn.children.get(1)?;
    if raw.kind == FormKind::Metadata {
        // ^meta name — the annotated form is the second child of Meta.
        raw.children.get(1)
    } else {
        Some(raw)
    }
}

// ── Legacy byte-scanning fallback ────────────────────────────────────────────
//
// Used when the AST parse fails (e.g., invalid source mid-edit).  These are
// the original string-surgery helpers from the pre-AST implementation.

fn apply_via_scan(source: &str, edit: &Edit) -> Result<String> {
    let src = source.as_bytes();
    match edit {
        Edit::ReplaceBody { fn_name, new_body } => {
            let (defn_start, defn_end) = require_defn(src, fn_name)?;
            let (_, args_end) = require_args_vec(src, defn_start, fn_name)?;
            Ok(format!(
                "{}\n  {new_body}\n{}",
                &source[..args_end],
                &source[defn_end - 1..],
            ))
        }

        Edit::PrependToBody { fn_name, form } => {
            let (defn_start, _) = require_defn(src, fn_name)?;
            let (_, args_end) = require_args_vec(src, defn_start, fn_name)?;
            Ok(format!(
                "{}\n  {form}{}",
                &source[..args_end],
                &source[args_end..],
            ))
        }

        Edit::WrapBody {
            fn_name,
            wrapper_prefix,
            wrapper_suffix,
        } => {
            let (defn_start, defn_end) = require_defn(src, fn_name)?;
            let (_, args_end) = require_args_vec(src, defn_start, fn_name)?;
            let body = source[args_end..defn_end - 1].trim();
            Ok(format!(
                "{}\n  {wrapper_prefix}{body}{wrapper_suffix}\n{}",
                &source[..args_end],
                &source[defn_end - 1..],
            ))
        }

        Edit::Rename { old_name, new_name } => {
            let (defn_start, _) = require_defn(src, old_name)?;
            let j = skip_ws(src, defn_start + 1);
            let kw_len = keyword_len(src, j);
            let name_start = skip_ws(src, j + kw_len);
            let name_end = name_start + old_name.len();
            Ok(format!(
                "{}{new_name}{}",
                &source[..name_start],
                &source[name_end..],
            ))
        }

        Edit::InsertAfter { anchor: None, form } => Ok(format!("{}\n\n{form}", source.trim_end())),

        Edit::InsertAfter {
            anchor: Some(name),
            form,
        } => {
            let (_, defn_end) = require_defn(src, name)?;
            Ok(format!(
                "{}\n\n{form}{}",
                &source[..defn_end],
                &source[defn_end..],
            ))
        }
    }
}

fn require_defn(src: &[u8], fn_name: &str) -> Result<(usize, usize)> {
    find_defn_span(src, fn_name.as_bytes()).ok_or_else(|| EditorError::NotFound(fn_name.to_owned()))
}

fn require_args_vec(src: &[u8], defn_start: usize, fn_name: &str) -> Result<(usize, usize)> {
    find_args_vec(src, defn_start)
        .ok_or_else(|| EditorError::InvalidEdit(format!("no argument vector found in {fn_name}")))
}

fn keyword_len(src: &[u8], pos: usize) -> usize {
    if src[pos..].starts_with(b"defn-") && is_delim(src, pos + 5) {
        5
    } else if src[pos..].starts_with(b"defn") && is_delim(src, pos + 4) {
        4
    } else {
        0
    }
}

fn skip_ws(src: &[u8], mut i: usize) -> usize {
    while i < src.len() && matches!(src[i], b' ' | b'\t' | b'\n' | b'\r' | b',') {
        i += 1;
    }
    i
}

fn is_delim(src: &[u8], pos: usize) -> bool {
    src.get(pos).is_none_or(|&c| {
        matches!(
            c,
            b' ' | b'\t'
                | b'\n'
                | b'\r'
                | b','
                | b'('
                | b')'
                | b'['
                | b']'
                | b'{'
                | b'}'
                | b';'
                | b'"'
        )
    })
}

fn form_end(src: &[u8], start: usize) -> Option<usize> {
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut i = start;
    while i < src.len() {
        if in_str {
            match src[i] {
                b'\\' => i += 1,
                b'"' => in_str = false,
                _ => {}
            }
        } else {
            match src[i] {
                b';' => {
                    while i < src.len() && src[i] != b'\n' {
                        i += 1
                    }
                }
                b'"' => in_str = true,
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i + 1);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

fn find_defn_span(src: &[u8], name: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    let mut in_str = false;
    while i < src.len() {
        if in_str {
            match src[i] {
                b'\\' => i += 1,
                b'"' => in_str = false,
                _ => {}
            }
            i += 1;
            continue;
        }
        match src[i] {
            b'"' => {
                in_str = true;
                i += 1;
                continue;
            }
            b';' => {
                while i < src.len() && src[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'(' => {
                let j = skip_ws(src, i + 1);
                let kw = keyword_len(src, j);
                if kw > 0 {
                    let k = skip_ws(src, j + kw);
                    if src[k..].starts_with(name) && is_delim(src, k + name.len()) {
                        if let Some(end) = form_end(src, i) {
                            return Some((i, end));
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_args_vec(src: &[u8], defn_start: usize) -> Option<(usize, usize)> {
    let mut depth: i32 = 1;
    let mut in_str = false;
    let mut i = defn_start + 1;
    while i < src.len() {
        if in_str {
            match src[i] {
                b'\\' => i += 1,
                b'"' => in_str = false,
                _ => {}
            }
            i += 1;
            continue;
        }
        match src[i] {
            b';' => {
                while i < src.len() && src[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => in_str = true,
            b'(' | b'{' => depth += 1,
            b'[' if depth == 1 => {
                let end = form_end(src, i)?;
                return Some((i, end));
            }
            b'[' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return None;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(src: &str, edit: Edit) -> String {
        Editor::new(src).apply(&[edit]).unwrap().source
    }

    const GREET: &str = "(defn greet [name]\n  (str \"Hello, \" name \"!\"))";

    #[test]
    fn replace_body() {
        let result = apply(
            GREET,
            Edit::ReplaceBody {
                fn_name: "greet".into(),
                new_body: "(str \"Hi, \" name)".into(),
            },
        );
        assert!(result.contains("(str \"Hi, \" name)"), "{result}");
        assert!(result.contains("[name]"), "{result}");
    }

    #[test]
    fn prepend_to_body() {
        let result = apply(
            GREET,
            Edit::PrependToBody {
                fn_name: "greet".into(),
                form: "(println \"calling greet\")".into(),
            },
        );
        assert!(result.contains("(println \"calling greet\")"), "{result}");
        assert!(result.contains("(str \"Hello, \" name"), "{result}");
    }

    #[test]
    fn wrap_body() {
        let result = apply(
            GREET,
            Edit::WrapBody {
                fn_name: "greet".into(),
                wrapper_prefix: "(do ".into(),
                wrapper_suffix: ")".into(),
            },
        );
        assert!(result.contains("(do "), "{result}");
        assert!(result.contains("(str \"Hello, \" name"), "{result}");
    }

    #[test]
    fn rename() {
        let result = apply(
            GREET,
            Edit::Rename {
                old_name: "greet".into(),
                new_name: "welcome".into(),
            },
        );
        assert!(result.contains("defn welcome"), "{result}");
        assert!(!result.contains("defn greet"), "{result}");
    }

    #[test]
    fn insert_after_anchor_none() {
        let result = apply(
            GREET,
            Edit::InsertAfter {
                anchor: None,
                form: "(defn bye [name] \"Bye!\")".into(),
            },
        );
        assert!(result.contains("defn greet"), "{result}");
        assert!(result.contains("defn bye"), "{result}");
        assert!(result.rfind("defn bye").unwrap() > result.rfind("defn greet").unwrap());
    }

    #[test]
    fn insert_after_anchor_some() {
        let src = "(defn a [] 1)\n\n(defn b [] 2)";
        let result = apply(
            src,
            Edit::InsertAfter {
                anchor: Some("a".into()),
                form: "(defn c [] 3)".into(),
            },
        );
        let pos_a = result.find("defn a").unwrap();
        let pos_c = result.find("defn c").unwrap();
        let pos_b = result.find("defn b").unwrap();
        assert!(pos_a < pos_c && pos_c < pos_b, "{result}");
    }

    #[test]
    fn not_found_returns_error() {
        let err = Editor::new(GREET)
            .apply(&[Edit::ReplaceBody {
                fn_name: "nope".into(),
                new_body: "x".into(),
            }])
            .unwrap_err();
        assert!(matches!(err, EditorError::NotFound(_)));
    }

    #[test]
    fn defn_inside_string_not_matched() {
        let src = "(def docs \"(defn fake [x] x)\")\n(defn real [x] x)";
        let result = apply(
            src,
            Edit::Rename {
                old_name: "real".into(),
                new_name: "actual".into(),
            },
        );
        assert!(result.contains("defn actual"), "{result}");
        assert!(result.contains("(defn fake [x] x)"), "{result}");
    }

    #[test]
    fn multiarity_replace_body() {
        let src = "(defn add\n  ([x] x)\n  ([x y] (+ x y)))";
        let result = apply(
            src,
            Edit::ReplaceBody {
                fn_name: "add".into(),
                new_body: "nil".into(),
            },
        );
        // The new body replaces everything between the first args vec and defn end.
        assert!(result.contains("nil"), "{result}");
        assert!(result.contains("defn add"), "{result}");
    }
}
