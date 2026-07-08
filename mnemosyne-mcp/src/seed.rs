//! Seeds the internal code repository with the built-in function library.
//!
//! `function_lookup` searches the git-backed code store, but the built-in
//! namespaces are embedded in the binary and loaded straight into the eval
//! runtime — without seeding, a fresh store is empty and an agent's first
//! lookups find nothing, even though `mnemosyne.core` and friends are sitting
//! loaded in the runtime. Syncing the embedded sources into the repository
//! makes the builtins discoverable through every lookup path (semantic,
//! full-text, exact) and gives them the same annotation and `ns/name@commit`
//! pin semantics as saved functions.
//!
//! The repository copy is kept pinned to the binary: when an embedded source
//! changes (a server upgrade) — or a builtin was edited in the store — the
//! file is rewritten so lookup always reflects what the runtime actually
//! loaded. Superseded versions stay reachable through git history and any
//! previously returned `@commit` pins.

use mnemosyne_code_storage::CodeRepository;
use mnemosyne_core_functions::embedded;

/// The built-in namespaces and where they live in the code store.
pub const BUILTIN_SOURCES: &[(&str, &str)] = &[
    ("src/mnemosyne/core.clj", embedded::CORE_CLJ),
    ("src/mnemosyne/templates.clj", embedded::TEMPLATES_CLJ),
    ("src/mnemosyne/shell.clj", embedded::SHELL_CLJ),
];

/// Bring the built-in namespaces in the repository up to date with the
/// sources embedded in this binary. All changes land in one commit; returns
/// the repo-relative paths that changed (empty when already in sync).
pub fn sync_builtins(repo: &CodeRepository) -> anyhow::Result<Vec<&'static str>> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| anyhow::anyhow!("code repository has no working directory"))?;

    let mut changed = Vec::new();
    for (rel_path, source) in BUILTIN_SOURCES {
        let current = std::fs::read_to_string(workdir.join(rel_path)).unwrap_or_default();
        if current != *source {
            repo.write_file(rel_path, source)?;
            changed.push(*rel_path);
        }
    }

    if !changed.is_empty() {
        repo.commit(&changed, "Sync built-in function library", None)?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer;
    use tempfile::TempDir;

    #[test]
    fn fresh_repo_gets_all_builtins_in_one_commit() {
        let dir = TempDir::new().unwrap();
        let repo = CodeRepository::init(dir.path()).unwrap();

        let changed = sync_builtins(&repo).unwrap();
        assert_eq!(changed.len(), BUILTIN_SOURCES.len());

        // Everything is committed and enumerable by the indexer.
        let fns = indexer::collect_functions(&repo);
        let names: Vec<&str> = fns.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"mnemosyne.core/deep-merge"), "{names:?}");
        assert!(names.contains(&"mnemosyne.shell/grep"), "{names:?}");
        assert!(
            names.contains(&"mnemosyne.templates/template:retry"),
            "{names:?}"
        );

        // Docstrings survive into the index entries.
        let grep = fns
            .iter()
            .find(|f| f.name == "mnemosyne.shell/grep")
            .unwrap();
        assert!(grep.docstring.as_deref().unwrap().contains("match maps"));
    }

    #[test]
    fn second_sync_is_a_no_op() {
        let dir = TempDir::new().unwrap();
        let repo = CodeRepository::init(dir.path()).unwrap();

        sync_builtins(&repo).unwrap();
        let head = repo.resolve_commit_hash("HEAD").unwrap();

        assert!(sync_builtins(&repo).unwrap().is_empty());
        assert_eq!(repo.resolve_commit_hash("HEAD").unwrap(), head);
    }

    #[test]
    fn drifted_builtin_is_restored() {
        let dir = TempDir::new().unwrap();
        let repo = CodeRepository::init(dir.path()).unwrap();
        sync_builtins(&repo).unwrap();

        let path = "src/mnemosyne/core.clj";
        repo.write_file(path, "(ns mnemosyne.core)\n(defn rogue [] :edited)\n")
            .unwrap();
        repo.commit(&[path], "edit a builtin", None).unwrap();

        let changed = sync_builtins(&repo).unwrap();
        assert_eq!(changed, vec![path]);

        let restored = std::fs::read_to_string(repo.workdir().unwrap().join(path)).unwrap();
        assert_eq!(restored, embedded::CORE_CLJ);
        // The edited version stays reachable through history.
        assert!(repo.resolve_commit_hash("HEAD~1").is_ok());
    }
}
