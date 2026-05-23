use std::collections::HashMap;
use std::path::PathBuf;

use mnemosyne_code_storage::CodeRepository;

use crate::{
    error::RegistryError,
    trust::{TrustLevel, TrustPolicy},
    vref::VersionedRef,
};

/// A resolved versioned symbol: source code plus provenance metadata.
#[derive(Debug, Clone)]
pub struct ResolvedSymbol {
    /// The canonical versioned reference that was resolved.
    pub vref: VersionedRef,
    /// Clojure source of the namespace (or extracted defn if a symbol was requested).
    pub source: String,
    /// Full 40-char commit SHA (normalised from whatever ref was given).
    pub commit_hash: String,
    /// PGP/SSH fingerprint of the signing key if the commit was signed.
    pub signature_fingerprint: Option<String>,
    /// `true` when the signature was verified against the system keyring.
    pub signature_valid: bool,
    /// Effective trust level determined by the policy.
    pub trust: TrustLevel,
}

/// Central registry for versioned Clojure symbols.
///
/// Resolves `[repo::]namespace[/symbol]@commit` references by:
/// 1. Opening or cloning the appropriate git repository
/// 2. Reading the namespace source at the pinned commit
/// 3. Optionally extracting a single defn
/// 4. Verifying the commit signature against the system keyring
/// 5. Applying the configured `TrustPolicy`
/// 6. Caching the result so repeat resolutions are free
pub struct SymbolRegistry {
    /// Named local repo aliases: alias → working-directory path.
    repos: HashMap<String, PathBuf>,
    /// Directory used as a cache for cloned external repos.
    repo_cache_dir: PathBuf,
    trust_policy: TrustPolicy,
    /// Resolved symbol cache: canonical vref string → result.
    cache: HashMap<String, ResolvedSymbol>,
}

impl SymbolRegistry {
    pub fn new(repo_cache_dir: impl Into<PathBuf>, trust_policy: TrustPolicy) -> Self {
        Self {
            repos: HashMap::new(),
            repo_cache_dir: repo_cache_dir.into(),
            trust_policy,
            cache: HashMap::new(),
        }
    }

    /// Register a named local repository alias.
    ///
    /// The `name` is matched against the leading segment of a namespace
    /// (e.g. `"mnemosyne"` matches `mnemosyne.core`), and also checked as
    /// the literal alias `"default"` if no closer match is found.
    pub fn register_repo(&mut self, name: impl Into<String>, path: impl Into<PathBuf>) {
        self.repos.insert(name.into(), path.into());
    }

    /// Parse `vref_str` and resolve it. Results are cached.
    pub fn resolve(&mut self, vref_str: &str) -> Result<ResolvedSymbol, RegistryError> {
        let vref = VersionedRef::parse(vref_str)?;
        self.resolve_ref(&vref)
    }

    /// Resolve a pre-parsed `VersionedRef`. Results are cached.
    pub fn resolve_ref(&mut self, vref: &VersionedRef) -> Result<ResolvedSymbol, RegistryError> {
        let cache_key = vref.canonical();
        if let Some(hit) = self.cache.get(&cache_key) {
            return Ok(hit.clone());
        }

        let repo = self.open_repo(vref)?;
        let commit_hash = repo.resolve_commit_hash(&vref.commit)?;

        let (fingerprint, sig_valid) =
            repo.commit_signature_status(&commit_hash)
                .unwrap_or_else(|e| {
                    tracing::warn!("signature check failed for {vref}: {e}");
                    (None, false)
                });

        let repo_key = vref.repo_url.as_deref().unwrap_or("local");
        let trust = self.trust_policy.resolve(repo_key, fingerprint.as_deref());

        if trust == TrustLevel::Deny {
            let reason = if self.trust_policy.require_signatures && fingerprint.is_none() {
                "unsigned code not permitted by policy".into()
            } else {
                "denied by trust policy".into()
            };
            return Err(RegistryError::TrustDenied(vref.to_string(), reason));
        }

        let source = self.read_symbol(&repo, vref, &commit_hash)?;

        let resolved = ResolvedSymbol {
            vref: vref.clone(),
            source,
            commit_hash,
            signature_fingerprint: fingerprint,
            signature_valid: sig_valid,
            trust,
        };
        self.cache.insert(cache_key, resolved.clone());
        Ok(resolved)
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn open_repo(&self, vref: &VersionedRef) -> Result<CodeRepository, RegistryError> {
        match &vref.repo_url {
            None => self.open_local_repo(vref),
            Some(url) => self.open_external_repo(url),
        }
    }

    fn open_local_repo(&self, vref: &VersionedRef) -> Result<CodeRepository, RegistryError> {
        let path = if self.repos.len() == 1 {
            // Only one repo registered — use it unconditionally.
            self.repos.values().next().unwrap().clone()
        } else {
            // Try the leading namespace segment, then "default".
            let ns_root = vref.namespace.split('.').next().unwrap_or(&vref.namespace);
            self.repos
                .get(ns_root)
                .or_else(|| self.repos.get("default"))
                .cloned()
                .ok_or_else(|| {
                    RegistryError::RepoNotFound(
                        vref.namespace.clone(),
                        format!("no repo registered for '{}'; call register_repo()", ns_root),
                    )
                })?
        };
        CodeRepository::open(path).map_err(RegistryError::Storage)
    }

    fn open_external_repo(&self, url: &str) -> Result<CodeRepository, RegistryError> {
        let local = self.external_cache_path(url);
        if local.exists() {
            let repo = CodeRepository::open(&local)?;
            if let Err(e) = repo.fetch_remote(url) {
                tracing::warn!("could not fetch updates from {url}: {e}");
            }
            Ok(repo)
        } else {
            std::fs::create_dir_all(&local)?;
            CodeRepository::clone(url, &local).map_err(RegistryError::Storage)
        }
    }

    fn external_cache_path(&self, url: &str) -> PathBuf {
        let safe: String = url
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.repo_cache_dir.join(safe)
    }

    fn read_symbol(
        &self,
        repo: &CodeRepository,
        vref: &VersionedRef,
        commit_hash: &str,
    ) -> Result<String, RegistryError> {
        let ns_path = namespace_to_path(&vref.namespace);
        let files = repo.files_at_rev(commit_hash)?;

        // Match against `ns_path`, `src/<ns_path>`, or any path ending in `/<ns_path>`.
        let file = files
            .iter()
            .find(|f| {
                f.path
                    .to_str()
                    .map(|p| {
                        p == ns_path
                            || p == format!("src/{ns_path}")
                            || p.ends_with(&format!("/{ns_path}"))
                    })
                    .unwrap_or(false)
            })
            .ok_or_else(|| {
                RegistryError::SymbolNotFound(vref.namespace.clone(), commit_hash.to_owned())
            })?;

        let full_source = file
            .content_str()
            .ok_or_else(|| {
                RegistryError::SymbolNotFound(
                    vref.namespace.clone(),
                    "file contains non-UTF-8 bytes".into(),
                )
            })?
            .to_owned();

        match &vref.symbol {
            None => Ok(full_source),
            Some(sym) => extract_defn(&full_source, sym).ok_or_else(|| {
                RegistryError::SymbolNotFound(
                    format!("{}/{}", vref.namespace, sym),
                    commit_hash.to_owned(),
                )
            }),
        }
    }
}

/// Convert a Clojure namespace identifier to a relative `.clj` file path.
///
/// `my.cool-ns` → `my/cool_ns.clj`
fn namespace_to_path(ns: &str) -> String {
    format!("{}.clj", ns.replace('.', "/").replace('-', "_"))
}

/// Extract the source of a specific `defn` form from `source` using balanced
/// parenthesis counting. Returns `None` if no matching `(defn <name> …)` is found.
fn extract_defn(source: &str, name: &str) -> Option<String> {
    let needle = format!("(defn {name}");
    let start = source.find(&needle)?;

    // Verify this isn't a prefix match (e.g. `defn foo-bar` when looking for `foo`).
    if let Some(ch) = source[start + needle.len()..].chars().next() {
        if ch != ' ' && ch != '\n' && ch != '\r' && ch != '[' && ch != '\t' {
            return None;
        }
    }

    let mut depth: i32 = 0;
    let mut end = start;
    for (i, ch) in source[start..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    if end == start {
        None
    } else {
        Some(source[start..end].to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_to_path_dots_and_hyphens() {
        assert_eq!(namespace_to_path("mnemosyne.core"), "mnemosyne/core.clj");
        assert_eq!(namespace_to_path("my.cool-ns"), "my/cool_ns.clj");
    }

    #[test]
    fn extract_simple_defn() {
        let src = r#"(ns foo)
(defn bar [x] (+ x 1))
(defn baz [y] y)"#;
        let extracted = extract_defn(src, "bar").unwrap();
        assert_eq!(extracted, "(defn bar [x] (+ x 1))");
    }

    #[test]
    fn extract_does_not_match_prefix() {
        let src = "(defn foobar [x] x)";
        assert!(extract_defn(src, "foo").is_none());
    }

    #[test]
    fn extract_nested_parens() {
        let src = "(defn f [x] (if (> x 0) (+ x 1) (- x 1)))";
        let extracted = extract_defn(src, "f").unwrap();
        assert_eq!(extracted, src);
    }
}
