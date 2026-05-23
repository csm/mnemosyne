//! Versioned Clojure symbol resolution for Mnemosyne.
//!
//! Every symbol reference in Mnemosyne is a *versioned ref*:
//!
//! ```text
//! [<repo-url>::]<namespace>[/<symbol>]@<commit>
//! ```
//!
//! The [`SymbolRegistry`] resolves these refs by:
//! - opening local or cloning external git repositories
//! - reading namespace source at the pinned commit
//! - optionally extracting a single `defn`
//! - verifying commit signatures via the system keyring
//! - applying a [`TrustPolicy`] before returning code to the caller
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use mnemosyne_symbol_registry::{SymbolRegistry, TrustPolicy};
//!
//! let mut registry = SymbolRegistry::new("/tmp/ext-repos", TrustPolicy::permissive());
//! registry.register_repo("mnemosyne", "/path/to/mnemosyne");
//!
//! let sym = registry.resolve("mnemosyne.core/deep-merge@a1b2c3d4").unwrap();
//! println!("{}", sym.source);
//! ```

pub mod error;
pub mod registry;
pub mod trust;
pub mod vref;

pub use error::RegistryError;
pub use registry::{ResolvedSymbol, SymbolRegistry};
pub use trust::{TrustLevel, TrustPolicy, TrustedKey};
pub use vref::VersionedRef;

pub type Result<T> = std::result::Result<T, RegistryError>;
