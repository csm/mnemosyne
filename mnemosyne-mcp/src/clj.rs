//! Lightweight, string/comment-aware scanning of Clojure source.
//!
//! This is not a reader: it only needs to find top-level definition forms,
//! their names, and their byte ranges so `save_function` can replace a
//! definition in place and the indexer can enumerate what a namespace file
//! defines. Parens inside string literals, comments, and character literals
//! are ignored.

/// Definition operators recognised at the head of a top-level form.
const DEF_OPS: &[&str] = &["defn", "defn-", "defmacro", "def"];

/// A top-level definition extracted from a namespace file.
#[derive(Debug, Clone, PartialEq)]
pub struct TopLevelDef {
    pub name: String,
    /// Docstring, when the form is a `defn`/`defn-`/`defmacro` with a string
    /// literal directly after the name.
    pub docstring: Option<String>,
    /// Full source text of the form, including the outer parens.
    pub source: String,
    /// Byte offset of the opening paren.
    pub start: usize,
    /// Byte offset one past the closing paren.
    pub end: usize,
}

/// Convert a Clojure namespace identifier to its conventional relative file
/// path: `my.cool-ns` → `my/cool_ns.clj`.
pub fn namespace_to_path(ns: &str) -> String {
    format!("{}.clj", ns.replace('.', "/").replace('-', "_"))
}

/// Inverse of [`namespace_to_path`] applied to a repo-relative file path:
/// `src/my/cool_ns.clj` → `my.cool-ns`. Returns `None` for non-Clojure files.
pub fn path_to_namespace(path: &str) -> Option<String> {
    let stripped = path.strip_prefix("src/").unwrap_or(path);
    let without_ext = stripped
        .strip_suffix(".clj")
        .or_else(|| stripped.strip_suffix(".cljc"))
        .or_else(|| stripped.strip_suffix(".cljs"))?;
    Some(without_ext.replace('/', ".").replace('_', "-"))
}

/// Enumerate all top-level `defn`/`defn-`/`defmacro`/`def` forms in `source`.
pub fn top_level_defs(source: &str) -> Vec<TopLevelDef> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b';' => i = skip_line(bytes, i),
            b'"' => i = skip_string(bytes, i),
            b'\\' => i += 2, // character literal: skip the escaped char
            b'(' => {
                let Some(end) = matching_paren(bytes, i) else {
                    break; // unbalanced source: stop scanning
                };
                if let Some(def) = parse_def(source, i, end) {
                    out.push(def);
                }
                i = end;
            }
            _ => i += 1,
        }
    }
    out
}

/// Replace the top-level definition named `name` in `existing` with
/// `new_source`, or append `new_source` when no such definition exists.
pub fn upsert_def(existing: &str, name: &str, new_source: &str) -> String {
    match top_level_defs(existing)
        .into_iter()
        .find(|d| d.name == name)
    {
        Some(def) => format!(
            "{}{}{}",
            &existing[..def.start],
            new_source.trim_end(),
            &existing[def.end..]
        ),
        None => {
            let mut s = existing.trim_end().to_owned();
            if !s.is_empty() {
                s.push_str("\n\n");
            }
            s.push_str(new_source.trim_end());
            s.push('\n');
            s
        }
    }
}

// ── Scanner internals ────────────────────────────────────────────────────────

/// Advance past a `;` comment to the start of the next line.
fn skip_line(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

/// Advance past a string literal whose opening quote is at `i`.
fn skip_string(bytes: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    i
}

/// Byte offset one past the paren matching the `(` at `open`, or `None` when
/// the source is unbalanced.
fn matching_paren(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b';' => i = skip_line(bytes, i),
            b'"' => i = skip_string(bytes, i),
            b'\\' => i += 2,
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => i += 1,
        }
    }
    None
}

/// If the form spanning `[start, end)` is a definition, extract its metadata.
fn parse_def(source: &str, start: usize, end: usize) -> Option<TopLevelDef> {
    let text = &source[start..end];
    let mut c = Cursor::new(text);
    c.expect(b'(')?;
    c.skip_ws();

    let op = c.read_symbol()?;
    if !DEF_OPS.contains(&op.as_str()) {
        return None;
    }

    c.skip_ws();
    c.skip_metadata();
    let name = c.read_symbol()?;

    // A string literal directly after the name of a fn-like form is a
    // docstring. (For plain `def` a string there may be the *value*, so we
    // don't claim it.)
    let docstring = if op != "def" {
        c.skip_ws();
        c.read_string_literal()
    } else {
        None
    };

    Some(TopLevelDef {
        name,
        docstring,
        source: text.to_owned(),
        start,
        end,
    })
}

/// Tiny byte cursor used only for picking apart the head of a definition form.
struct Cursor<'a> {
    bytes: &'a [u8],
    i: usize,
}

impl<'a> Cursor<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            i: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.i).copied()
    }

    fn expect(&mut self, b: u8) -> Option<()> {
        if self.peek() == Some(b) {
            self.i += 1;
            Some(())
        } else {
            None
        }
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b.is_ascii_whitespace() || b == b',' {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    /// Skip `^meta` markers (`^:private`, `^String`, `^{…}`) plus trailing
    /// whitespace, so the cursor lands on the definition name.
    fn skip_metadata(&mut self) {
        while self.peek() == Some(b'^') {
            self.i += 1;
            if self.peek() == Some(b'{') {
                let mut depth = 0i32;
                while let Some(b) = self.peek() {
                    match b {
                        b'"' => self.i = skip_string(self.bytes, self.i),
                        b'{' => {
                            depth += 1;
                            self.i += 1;
                        }
                        b'}' => {
                            depth -= 1;
                            self.i += 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => self.i += 1,
                    }
                }
            } else {
                self.read_symbol();
            }
            self.skip_ws();
        }
    }

    fn read_symbol(&mut self) -> Option<String> {
        let start = self.i;
        while let Some(b) = self.peek() {
            if b.is_ascii_whitespace()
                || matches!(
                    b,
                    b',' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'"' | b';'
                )
            {
                break;
            }
            self.i += 1;
        }
        if self.i == start {
            return None;
        }
        std::str::from_utf8(&self.bytes[start..self.i])
            .ok()
            .map(str::to_owned)
    }

    /// Read a string literal at the cursor, processing common escapes.
    /// Returns `None` (without advancing) when the cursor is not at `"`.
    fn read_string_literal(&mut self) -> Option<String> {
        if self.peek() != Some(b'"') {
            return None;
        }
        let end = skip_string(self.bytes, self.i);
        let raw = std::str::from_utf8(&self.bytes[self.i + 1..end.saturating_sub(1)]).ok()?;
        self.i = end;

        let mut out = String::with_capacity(raw.len());
        let mut chars = raw.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some(other) => out.push(other),
                    None => break,
                }
            } else {
                out.push(ch);
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_path_round_trip() {
        assert_eq!(namespace_to_path("my.cool-ns"), "my/cool_ns.clj");
        assert_eq!(
            path_to_namespace("src/my/cool_ns.clj"),
            Some("my.cool-ns".into())
        );
        assert_eq!(path_to_namespace("meta/readme.md"), None);
    }

    #[test]
    fn finds_multiple_defs_with_docstrings() {
        let src = r#"(ns foo.bar)

(defn add
  "Add two numbers."
  [a b]
  (+ a b))

(def answer 42)

(defn- helper [x] (* x 2))
"#;
        let defs = top_level_defs(src);
        assert_eq!(defs.len(), 3);
        assert_eq!(defs[0].name, "add");
        assert_eq!(defs[0].docstring.as_deref(), Some("Add two numbers."));
        assert_eq!(defs[1].name, "answer");
        assert_eq!(defs[1].docstring, None);
        assert_eq!(defs[2].name, "helper");
        assert!(defs[2].source.starts_with("(defn- helper"));
    }

    #[test]
    fn parens_in_strings_and_comments_are_ignored() {
        let src = r#";; a comment with (defn fake [x] x)
(defn real
  "Docstring with ) and ( inside."
  [s]
  (str s "))(("))
"#;
        let defs = top_level_defs(src);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "real");
        assert!(defs[0].source.ends_with(r#"(str s "))(("))"#));
    }

    #[test]
    fn char_literals_do_not_break_balance() {
        let src = r"(defn parens [] [\( \)])";
        let defs = top_level_defs(src);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].source, src);
    }

    #[test]
    fn metadata_before_name_is_skipped() {
        let src = "(defn ^:private ^{:added \"1.0\"} secret [x] x)";
        let defs = top_level_defs(src);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "secret");
    }

    #[test]
    fn upsert_replaces_existing_definition() {
        let existing = "(ns foo)\n\n(defn add [a b] (+ a b))\n\n(defn sub [a b] (- a b))\n";
        let updated = upsert_def(
            existing,
            "add",
            "(defn add\n  \"v2\"\n  [a b]\n  (+ a b 0))",
        );
        assert!(updated.contains("\"v2\""));
        assert!(!updated.contains("(defn add [a b] (+ a b))"));
        // The sibling function is untouched.
        assert!(updated.contains("(defn sub [a b] (- a b))"));
        assert_eq!(top_level_defs(&updated).len(), 2);
    }

    #[test]
    fn upsert_appends_new_definition() {
        let existing = "(ns foo)\n";
        let updated = upsert_def(existing, "mul", "(defn mul [a b] (* a b))");
        assert!(updated.ends_with("(defn mul [a b] (* a b))\n"));
        assert!(updated.starts_with("(ns foo)\n\n"));
    }

    #[test]
    fn non_definition_forms_are_skipped() {
        let src = "(ns foo)\n(println \"side effect\")\n(comment (defn nested [] nil))\n";
        assert!(top_level_defs(src).is_empty());
    }
}
