//! Episodic memory for Mnemosyne agent sessions.
//!
//! Every interaction — user messages, tool calls, eval results, edits — is
//! appended to an append-only log stored as newline-delimited JSON under a
//! caller-supplied base directory. The full session can be exported as EDN
//! for human inspection or loading into a Clojure REPL.
//!
//! # Quick start
//!
//! ```no_run
//! use mnemosyne_memory::{MemoryStore, EpisodeKind};
//! use std::path::Path;
//!
//! let mut store = MemoryStore::create(Path::new("/tmp/mnemosyne-memory")).unwrap();
//! store.log(EpisodeKind::UserMessage { content: "find retry helpers".into() }).unwrap();
//! println!("{}", store.export_edn());
//! ```

pub mod edn;
pub mod episode;
pub mod error;
pub mod store;

pub use episode::{Episode, EpisodeKind, SessionId};
pub use error::MemoryError;
pub use store::MemoryStore;

pub type Result<T> = std::result::Result<T, MemoryError>;
