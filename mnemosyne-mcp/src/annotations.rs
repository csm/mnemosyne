//! Function annotations: descriptions and use cases stored as EDN sidecar
//! files inside the code repository.
//!
//! A function `my.ns/frob` gets its annotation at `meta/my/ns/frob.edn`:
//!
//! ```clojure
//! {:description "Frobnicates the widget."
//!  :use-cases ["normalising legacy input"
//!              "pre-flight validation"]}
//! ```
//!
//! Annotations live in git next to the code they describe, so they version,
//! diff, and replicate with it. The reader below is deliberately minimal — it
//! parses exactly the shape this module writes (a map of keyword to string /
//! vector-of-string / nil), not general EDN.

/// Description and use cases attached to a saved function.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Annotation {
    pub description: Option<String>,
    pub use_cases: Vec<String>,
}

impl Annotation {
    pub fn is_empty(&self) -> bool {
        self.description.is_none() && self.use_cases.is_empty()
    }

    /// Merge new fields in: a new description replaces the old one; use cases
    /// are appended, skipping exact duplicates.
    pub fn merge(&mut self, description: Option<String>, use_cases: Vec<String>) {
        if description.is_some() {
            self.description = description;
        }
        for uc in use_cases {
            if !self.use_cases.contains(&uc) {
                self.use_cases.push(uc);
            }
        }
    }

    pub fn to_edn(&self) -> String {
        let mut pairs = Vec::new();
        if let Some(d) = &self.description {
            pairs.push(format!(":description {}", edn_str(d)));
        }
        if !self.use_cases.is_empty() {
            let items: Vec<String> = self.use_cases.iter().map(|s| edn_str(s)).collect();
            pairs.push(format!(":use-cases [{}]", items.join("\n             ")));
        }
        format!("{{{}}}\n", pairs.join("\n "))
    }

    pub fn from_edn(s: &str) -> Result<Self, String> {
        let mut p = EdnCursor::new(s);
        p.skip_ws();
        p.expect(b'{')?;

        let mut ann = Annotation::default();
        loop {
            p.skip_ws();
            match p.peek() {
                Some(b'}') => {
                    p.advance();
                    break;
                }
                None => return Err("unterminated map".into()),
                _ => {}
            }
            let key = p.read_keyword()?;
            p.skip_ws();
            match key.as_str() {
                "description" => ann.description = p.read_string_or_nil()?,
                "use-cases" => ann.use_cases = p.read_string_vec()?,
                _ => p.skip_value()?,
            }
        }
        Ok(ann)
    }
}

/// Repo-relative path of the annotation sidecar for `ns/name`.
pub fn annotation_rel_path(ns: &str, name: &str) -> String {
    let ns_dir = ns.replace('.', "/").replace('-', "_");
    format!("meta/{ns_dir}/{name}.edn")
}

fn edn_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ── Minimal EDN reader ───────────────────────────────────────────────────────

struct EdnCursor<'a> {
    bytes: &'a [u8],
    i: usize,
}

impl<'a> EdnCursor<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            i: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.i).copied()
    }

    fn advance(&mut self) {
        self.i += 1;
    }

    fn expect(&mut self, b: u8) -> Result<(), String> {
        if self.peek() == Some(b) {
            self.advance();
            Ok(())
        } else {
            Err(format!("expected '{}' at offset {}", b as char, self.i))
        }
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            match b {
                b if b.is_ascii_whitespace() || b == b',' => self.advance(),
                b';' => {
                    while self.peek().is_some_and(|c| c != b'\n') {
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    fn read_keyword(&mut self) -> Result<String, String> {
        self.expect(b':')?;
        let start = self.i;
        while let Some(b) = self.peek() {
            if b.is_ascii_whitespace()
                || matches!(b, b',' | b'{' | b'}' | b'[' | b']' | b'(' | b')' | b'"')
            {
                break;
            }
            self.advance();
        }
        if self.i == start {
            return Err(format!("empty keyword at offset {start}"));
        }
        String::from_utf8(self.bytes[start..self.i].to_vec()).map_err(|e| e.to_string())
    }

    fn read_string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err("unterminated string".into()),
                Some(b'"') => {
                    self.advance();
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.advance();
                    match self.peek() {
                        Some(b'n') => out.push('\n'),
                        Some(b't') => out.push('\t'),
                        Some(b'r') => out.push('\r'),
                        Some(other) => out.push(other as char),
                        None => return Err("dangling escape".into()),
                    }
                    self.advance();
                }
                Some(_) => {
                    // Push the full UTF-8 char, not the raw byte.
                    let rest = std::str::from_utf8(&self.bytes[self.i..])
                        .map_err(|e| e.to_string())?;
                    let ch = rest.chars().next().unwrap();
                    out.push(ch);
                    self.i += ch.len_utf8();
                }
            }
        }
    }

    fn read_string_or_nil(&mut self) -> Result<Option<String>, String> {
        if self.bytes[self.i..].starts_with(b"nil") {
            self.i += 3;
            return Ok(None);
        }
        self.read_string().map(Some)
    }

    fn read_string_vec(&mut self) -> Result<Vec<String>, String> {
        self.expect(b'[')?;
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b']') => {
                    self.advance();
                    return Ok(out);
                }
                Some(b'"') => out.push(self.read_string()?),
                Some(other) => {
                    return Err(format!("expected string in vector, got '{}'", other as char))
                }
                None => return Err("unterminated vector".into()),
            }
        }
    }

    /// Skip one value of any supported shape (used for unknown keys).
    fn skip_value(&mut self) -> Result<(), String> {
        self.skip_ws();
        match self.peek() {
            Some(b'"') => self.read_string().map(|_| ()),
            Some(b'[') | Some(b'{') | Some(b'(') => {
                let (open, close) = match self.peek().unwrap() {
                    b'[' => (b'[', b']'),
                    b'{' => (b'{', b'}'),
                    _ => (b'(', b')'),
                };
                let mut depth = 0i32;
                while let Some(b) = self.peek() {
                    if b == b'"' {
                        self.read_string()?;
                        continue;
                    }
                    if b == open {
                        depth += 1;
                    } else if b == close {
                        depth -= 1;
                        if depth == 0 {
                            self.advance();
                            return Ok(());
                        }
                    }
                    self.advance();
                }
                Err("unterminated collection".into())
            }
            Some(_) => {
                // Bare token: nil, number, keyword, symbol.
                while let Some(b) = self.peek() {
                    if b.is_ascii_whitespace()
                        || matches!(b, b',' | b'{' | b'}' | b'[' | b']' | b'(' | b')')
                    {
                        break;
                    }
                    self.advance();
                }
                Ok(())
            }
            None => Err("expected value".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edn_round_trip() {
        let ann = Annotation {
            description: Some("Retries an op with \"backoff\".\nSecond line.".into()),
            use_cases: vec!["transient network errors".into(), "rate limits".into()],
        };
        let edn = ann.to_edn();
        let parsed = Annotation::from_edn(&edn).unwrap();
        assert_eq!(parsed, ann);
    }

    #[test]
    fn empty_annotation_round_trip() {
        let ann = Annotation::default();
        assert_eq!(Annotation::from_edn(&ann.to_edn()).unwrap(), ann);
    }

    #[test]
    fn unknown_keys_are_skipped() {
        let edn = r#"{:author "someone" :tags [:a :b] :description "kept" :extra {:x 1}}"#;
        let parsed = Annotation::from_edn(edn).unwrap();
        assert_eq!(parsed.description.as_deref(), Some("kept"));
        assert!(parsed.use_cases.is_empty());
    }

    #[test]
    fn merge_replaces_description_and_dedupes_use_cases() {
        let mut ann = Annotation {
            description: Some("old".into()),
            use_cases: vec!["a".into()],
        };
        ann.merge(Some("new".into()), vec!["a".into(), "b".into()]);
        assert_eq!(ann.description.as_deref(), Some("new"));
        assert_eq!(ann.use_cases, vec!["a".to_owned(), "b".to_owned()]);

        ann.merge(None, vec![]);
        assert_eq!(ann.description.as_deref(), Some("new"));
    }

    #[test]
    fn sidecar_path_follows_namespace_convention() {
        assert_eq!(
            annotation_rel_path("my.cool-ns", "frob!"),
            "meta/my/cool_ns/frob!.edn"
        );
    }
}
