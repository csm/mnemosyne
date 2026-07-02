//! Builds per-function index entries from the internal code repository.
//!
//! Unlike `CodeIndex::index_repository` (which indexes whole files), this
//! walks every top-level definition so search results carry real function
//! names, docstrings, and bodies. Annotation sidecars under `meta/` are
//! merged into the docstring field so descriptions and use cases are
//! searchable by both full-text and semantic lookup.

use std::collections::HashMap;

use mnemosyne_code_search::IndexedFunction;
use mnemosyne_code_storage::CodeRepository;

use crate::annotations::Annotation;
use crate::clj;

/// Repo alias used for everything in the internal code store.
pub const INTERNAL_REPO_NAME: &str = "internal";

/// Collect one `IndexedFunction` per top-level definition at HEAD.
///
/// Returns an empty list for a repository with no commits yet.
pub fn collect_functions(repo: &CodeRepository) -> Vec<IndexedFunction> {
    let files = match repo.files_at_rev("HEAD") {
        Ok(f) => f,
        Err(_) => return Vec::new(), // fresh repo: nothing committed yet
    };

    // Pass 1: annotation sidecars, keyed by (namespace-dir-path, fn-name).
    let mut annotations: HashMap<(String, String), Annotation> = HashMap::new();
    for f in &files {
        let Some(path) = f.path.to_str() else {
            continue;
        };
        let Some(rest) = path.strip_prefix("meta/") else {
            continue;
        };
        let Some(rest) = rest.strip_suffix(".edn") else {
            continue;
        };
        let Some((ns_dir, name)) = rest.rsplit_once('/') else {
            continue;
        };
        let Some(content) = f.content_str() else {
            continue;
        };
        match Annotation::from_edn(content) {
            Ok(ann) => {
                annotations.insert((ns_dir.to_owned(), name.to_owned()), ann);
            }
            Err(e) => tracing::warn!(path, "skipping unreadable annotation: {e}"),
        }
    }

    // Pass 2: definitions in Clojure sources.
    let mut out = Vec::new();
    for f in &files {
        let Some(path) = f.path.to_str() else {
            continue;
        };
        if path.starts_with("meta/") {
            continue;
        }
        let is_clj = f
            .path
            .extension()
            .is_some_and(|e| e == "clj" || e == "cljc" || e == "cljs");
        if !is_clj {
            continue;
        }
        let Some(content) = f.content_str() else {
            continue;
        };

        let ns = clj::path_to_namespace(path);
        // Sidecar keys use the namespace directory layout (underscores).
        let ns_dir = ns
            .as_deref()
            .map(|n| n.replace('.', "/").replace('-', "_"))
            .unwrap_or_default();

        for def in clj::top_level_defs(content) {
            let ann = annotations.get(&(ns_dir.clone(), def.name.clone()));
            let qualified = match &ns {
                Some(n) => format!("{n}/{}", def.name),
                None => def.name.clone(),
            };
            out.push(IndexedFunction {
                repo: INTERNAL_REPO_NAME.to_owned(),
                file_path: path.to_owned(),
                name: qualified,
                docstring: merged_docstring(def.docstring.as_deref(), ann),
                body: def.source,
            });
        }
    }
    out
}

/// Combine a parsed docstring with annotation text into one searchable field.
pub fn merged_docstring(parsed: Option<&str>, ann: Option<&Annotation>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(d) = parsed {
        parts.push(d.to_owned());
    }
    if let Some(a) = ann {
        if let Some(d) = &a.description {
            parts.push(d.clone());
        }
        if !a.use_cases.is_empty() {
            parts.push(format!("Use cases: {}", a.use_cases.join("; ")));
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn repo_with(files: &[(&str, &str)]) -> (TempDir, CodeRepository) {
        let dir = TempDir::new().unwrap();
        let repo = CodeRepository::init(dir.path()).unwrap();
        for (path, content) in files {
            repo.write_file(path, content).unwrap();
        }
        let paths: Vec<&str> = files.iter().map(|(p, _)| *p).collect();
        repo.commit(&paths, "seed", None).unwrap();
        (dir, repo)
    }

    #[test]
    fn empty_repo_yields_no_functions() {
        let dir = TempDir::new().unwrap();
        let repo = CodeRepository::init(dir.path()).unwrap();
        assert!(collect_functions(&repo).is_empty());
    }

    #[test]
    fn collects_qualified_functions_with_annotations() {
        let (_dir, repo) = repo_with(&[
            (
                "src/my/util.clj",
                "(ns my.util)\n\n(defn add\n  \"Adds.\"\n  [a b]\n  (+ a b))\n\n(def answer 42)\n",
            ),
            (
                "meta/my/util/add.edn",
                "{:description \"Sums two numbers.\"\n :use-cases [\"math\"]}\n",
            ),
        ]);

        let fns = collect_functions(&repo);
        assert_eq!(fns.len(), 2);

        let add = fns.iter().find(|f| f.name == "my.util/add").unwrap();
        let doc = add.docstring.as_deref().unwrap();
        assert!(doc.contains("Adds."));
        assert!(doc.contains("Sums two numbers."));
        assert!(doc.contains("Use cases: math"));
        assert!(add.body.starts_with("(defn add"));

        let answer = fns.iter().find(|f| f.name == "my.util/answer").unwrap();
        assert_eq!(answer.docstring, None);
    }
}
