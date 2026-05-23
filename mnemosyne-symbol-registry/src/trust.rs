use std::collections::HashMap;

/// Execution trust granted to loaded code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustLevel {
    /// Execute without additional restrictions.
    Full,
    /// Execute inside a capability sandbox (enforced by the execution engine).
    Sandboxed,
    /// Refuse to load; any attempt returns `RegistryError::TrustDenied`.
    Deny,
}

/// A public signing key whose signatures elevate trust.
#[derive(Debug, Clone)]
pub struct TrustedKey {
    /// PGP fingerprint or SSH key fingerprint string used for matching.
    pub fingerprint: String,
    /// Human-readable label (author name, organisation, etc.).
    pub owner: String,
    /// Trust level granted when this key's signature is valid.
    pub trust: TrustLevel,
}

/// Policy that governs how much to trust versioned code at load time.
///
/// Resolution order (first match wins):
/// 1. Explicit per-repo override in `repo_trust`
/// 2. Valid signature by a key in `trusted_keys`
/// 3. `require_signatures` check (unsigned → `Deny` when enabled)
/// 4. `default_trust`
#[derive(Debug, Clone)]
pub struct TrustPolicy {
    /// Fallback trust for code that doesn't match a more specific rule.
    pub default_trust: TrustLevel,
    /// Per-repo overrides keyed by repo URL or registered alias.
    pub repo_trust: HashMap<String, TrustLevel>,
    /// Keys whose valid signatures are accepted.
    pub trusted_keys: Vec<TrustedKey>,
    /// When `true`, code with no valid signature is treated as `Deny`.
    pub require_signatures: bool,
}

impl Default for TrustPolicy {
    fn default() -> Self {
        Self {
            default_trust: TrustLevel::Sandboxed,
            repo_trust: HashMap::new(),
            trusted_keys: Vec::new(),
            require_signatures: false,
        }
    }
}

impl TrustPolicy {
    /// Permissive policy — all code is `Full` trust, signatures optional.
    ///
    /// Suitable for single-agent development environments.
    pub fn permissive() -> Self {
        Self {
            default_trust: TrustLevel::Full,
            require_signatures: false,
            ..Default::default()
        }
    }

    /// Strict policy — every symbol must carry a valid signature from one of
    /// `trusted_keys`; unsigned or unknown-key code is `Deny`.
    pub fn strict(trusted_keys: Vec<TrustedKey>) -> Self {
        Self {
            default_trust: TrustLevel::Deny,
            trusted_keys,
            require_signatures: true,
            ..Default::default()
        }
    }

    /// Resolve the effective trust level.
    ///
    /// `repo_key` is the repo URL or registered alias.
    /// `fingerprint` is the verified signing-key fingerprint, if any.
    pub fn resolve(&self, repo_key: &str, fingerprint: Option<&str>) -> TrustLevel {
        // 1. Explicit repo override
        if let Some(level) = self.repo_trust.get(repo_key) {
            return level.clone();
        }

        // 2. Valid signature by a trusted key
        if let Some(fp) = fingerprint {
            for key in &self.trusted_keys {
                if key.fingerprint == fp {
                    return key.trust.clone();
                }
            }
        }

        // 3. Signature requirement gate
        if self.require_signatures && fingerprint.is_none() {
            return TrustLevel::Deny;
        }

        // 4. Default
        self.default_trust.clone()
    }
}
