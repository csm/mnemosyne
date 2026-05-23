use crate::RegistryError;

/// A parsed versioned Clojure symbol reference.
///
/// Syntax (Clojurust extension):
/// ```text
/// [<repo-url>::]<namespace>[/<symbol>]@<commit>
/// ```
///
/// Examples:
/// - `mnemosyne.core/deep-merge@a1b2c3d4`  — single function, local repo
/// - `mnemosyne.core@a1b2c3d4`             — whole namespace, local repo
/// - `https://github.com/u/r::app.util/parse@deadbeef` — external repo
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VersionedRef {
    /// External repository URL (`https://…`, `git@…`). `None` means a locally
    /// registered repo is used.
    pub repo_url: Option<String>,
    /// Clojure namespace (e.g. `"mnemosyne.core"`).
    pub namespace: String,
    /// Specific var within the namespace. `None` loads the whole namespace.
    pub symbol: Option<String>,
    /// Git commit hash (should be a full 40-char SHA for a stable pin).
    pub commit: String,
}

impl VersionedRef {
    /// Parse a versioned reference string.
    pub fn parse(s: &str) -> Result<Self, RegistryError> {
        let bad = |msg: &str| RegistryError::InvalidRef(s.to_owned(), msg.to_owned());

        // Split optional repo-url prefix at `::`
        let (repo_url, remainder) = match s.find("::") {
            Some(idx) => (Some(s[..idx].to_owned()), &s[idx + 2..]),
            None => (None, s),
        };

        // Split at the last `@` to get commit hash
        let at = remainder.rfind('@').ok_or_else(|| bad("missing '@<commit>'"))?;
        let sym_part = &remainder[..at];
        let commit = remainder[at + 1..].to_owned();

        if commit.is_empty() {
            return Err(bad("commit hash is empty"));
        }

        // Split sym_part on the first `/`
        let (namespace, symbol) = match sym_part.find('/') {
            Some(slash) => {
                let sym = sym_part[slash + 1..].to_owned();
                if sym.is_empty() {
                    return Err(bad("symbol name after '/' is empty"));
                }
                (sym_part[..slash].to_owned(), Some(sym))
            }
            None => (sym_part.to_owned(), None),
        };

        if namespace.is_empty() {
            return Err(bad("namespace is empty"));
        }

        Ok(Self { repo_url, namespace, symbol, commit })
    }

    /// Canonical string representation (round-trips through `parse`).
    pub fn canonical(&self) -> String {
        let mut s = String::new();
        if let Some(url) = &self.repo_url {
            s.push_str(url);
            s.push_str("::");
        }
        s.push_str(&self.namespace);
        if let Some(sym) = &self.symbol {
            s.push('/');
            s.push_str(sym);
        }
        s.push('@');
        s.push_str(&self.commit);
        s
    }
}

impl std::fmt::Display for VersionedRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.canonical())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_symbol_ref() {
        let vref = VersionedRef::parse("mnemosyne.core/deep-merge@a1b2c3d4").unwrap();
        assert_eq!(vref.repo_url, None);
        assert_eq!(vref.namespace, "mnemosyne.core");
        assert_eq!(vref.symbol, Some("deep-merge".into()));
        assert_eq!(vref.commit, "a1b2c3d4");
    }

    #[test]
    fn parse_namespace_ref() {
        let vref = VersionedRef::parse("mnemosyne.core@deadbeef").unwrap();
        assert_eq!(vref.symbol, None);
        assert_eq!(vref.commit, "deadbeef");
    }

    #[test]
    fn parse_external_ref() {
        let vref =
            VersionedRef::parse("https://github.com/u/r::app.util/parse@cafe1234").unwrap();
        assert_eq!(vref.repo_url, Some("https://github.com/u/r".into()));
        assert_eq!(vref.namespace, "app.util");
        assert_eq!(vref.symbol, Some("parse".into()));
    }

    #[test]
    fn round_trip() {
        let s = "https://github.com/u/r::app.util/parse@cafe1234";
        assert_eq!(VersionedRef::parse(s).unwrap().canonical(), s);
    }

    #[test]
    fn missing_at_is_error() {
        assert!(VersionedRef::parse("mnemosyne.core/foo").is_err());
    }

    #[test]
    fn empty_commit_is_error() {
        assert!(VersionedRef::parse("mnemosyne.core/foo@").is_err());
    }
}
