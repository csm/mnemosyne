use crate::{Result, StorageError};
use git2::Repository as GitRepo;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Where a repository comes from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RepoSource {
    Local(PathBuf),
    Remote { url: String, local_path: PathBuf },
}

/// A single file blob retrieved from the repository tree.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub content: Vec<u8>,
}

impl FileEntry {
    pub fn content_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.content).ok()
    }
}

/// Author identity used when creating commits.
#[derive(Debug, Clone)]
pub struct CommitAuthor {
    pub name: String,
    pub email: String,
}

impl Default for CommitAuthor {
    fn default() -> Self {
        Self {
            name: "Mnemosyne".into(),
            email: "mnemosyne@local".into(),
        }
    }
}

/// Thin wrapper around a git repository with helpers for walking file trees
/// and writing new commits.
pub struct CodeRepository {
    inner: GitRepo,
    pub source: RepoSource,
}

impl CodeRepository {
    /// Open an existing local repository.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let inner = GitRepo::open(path)?;
        Ok(Self {
            inner,
            source: RepoSource::Local(path.to_owned()),
        })
    }

    /// Initialise a brand-new repository at `path` (like `git init`), creating
    /// the directory if needed.
    pub fn init(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        std::fs::create_dir_all(path)?;
        tracing::info!(path = %path.display(), "initialising repository");
        let inner = GitRepo::init(path)?;
        Ok(Self {
            inner,
            source: RepoSource::Local(path.to_owned()),
        })
    }

    /// Open the repository at `path`, initialising a fresh one if none exists.
    pub fn open_or_init(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        match Self::open(path) {
            Ok(repo) => Ok(repo),
            Err(_) => Self::init(path),
        }
    }

    /// Clone a remote repository to `dest`.
    pub fn clone(url: &str, dest: impl AsRef<Path>) -> Result<Self> {
        let dest = dest.as_ref();
        tracing::info!(url, dest = %dest.display(), "cloning repository");
        let inner = GitRepo::clone(url, dest)?;
        Ok(Self {
            inner,
            source: RepoSource::Remote {
                url: url.to_owned(),
                local_path: dest.to_owned(),
            },
        })
    }

    /// Return the local working directory path, or `None` for a bare repo.
    pub fn workdir(&self) -> Option<&Path> {
        self.inner.workdir()
    }

    // ── Read ──────────────────────────────────────────────────────────────────

    /// Iterate all blobs reachable from `rev` (e.g. `"HEAD"`).
    pub fn files_at_rev(&self, rev: &str) -> Result<Vec<FileEntry>> {
        let obj = self
            .inner
            .revparse_single(rev)
            .map_err(|_| StorageError::InvalidRef(rev.to_owned()))?;
        let commit = obj
            .peel_to_commit()
            .map_err(|_| StorageError::InvalidRef(rev.to_owned()))?;
        let tree = commit.tree()?;

        let mut entries = Vec::new();
        tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
            if entry.kind() == Some(git2::ObjectType::Blob) {
                if let Ok(obj) = entry.to_object(&self.inner) {
                    if let Some(blob) = obj.as_blob() {
                        let path = PathBuf::from(root).join(entry.name().unwrap_or(""));
                        entries.push(FileEntry {
                            path,
                            content: blob.content().to_vec(),
                        });
                    }
                }
            }
            git2::TreeWalkResult::Ok
        })?;

        Ok(entries)
    }

    // ── Write ─────────────────────────────────────────────────────────────────

    /// Write `content` to `rel_path` inside the working directory, creating
    /// any missing parent directories. Does **not** stage or commit.
    pub fn write_file(&self, rel_path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> Result<()> {
        let workdir = self.inner.workdir().ok_or(StorageError::NoWorkdir)?;
        let abs = workdir.join(rel_path.as_ref());
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(abs, content.as_ref())?;
        Ok(())
    }

    /// Stage `rel_paths` and create a commit on the current branch.
    ///
    /// `author` defaults to `CommitAuthor::default()` when `None`. Falls back
    /// to the repo's configured identity (from `.git/config`) if present.
    ///
    /// Returns the OID of the new commit.
    pub fn commit(
        &self,
        rel_paths: &[impl AsRef<Path>],
        message: &str,
        author: Option<&CommitAuthor>,
    ) -> Result<git2::Oid> {
        let mut index = self.inner.index()?;

        for p in rel_paths {
            index.add_path(p.as_ref())?;
        }
        index.write()?;

        let tree_oid = index.write_tree()?;
        let tree = self.inner.find_tree(tree_oid)?;

        let sig = match author {
            Some(a) => git2::Signature::now(&a.name, &a.email)?,
            None => self.inner.signature().unwrap_or_else(|_| {
                let d = CommitAuthor::default();
                git2::Signature::now(&d.name, &d.email).unwrap()
            }),
        };

        // The first commit has no parent; all others point at HEAD.
        let parent = self.inner.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();

        let oid = self
            .inner
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)?;
        tracing::info!(oid = %oid, "committed {} path(s)", rel_paths.len());
        Ok(oid)
    }

    // ── Version / signature helpers ───────────────────────────────────────────

    /// Resolve any git ref (branch, tag, short or full SHA) to its canonical
    /// 40-character commit SHA.
    pub fn resolve_commit_hash(&self, rev: &str) -> Result<String> {
        let obj = self
            .inner
            .revparse_single(rev)
            .map_err(|_| StorageError::InvalidRef(rev.to_owned()))?;
        let commit = obj
            .peel_to_commit()
            .map_err(|_| StorageError::InvalidRef(rev.to_owned()))?;
        Ok(commit.id().to_string())
    }

    /// Read the raw bytes of a single file at the given commit hash.
    pub fn read_file_at_commit(&self, hash: &str, rel_path: &str) -> Result<Vec<u8>> {
        let files = self.files_at_rev(hash)?;
        files
            .into_iter()
            .find(|f| f.path.to_str().map(|p| p == rel_path).unwrap_or(false))
            .map(|f| f.content)
            .ok_or_else(|| StorageError::NotFound(rel_path.to_owned()))
    }

    /// Return `(fingerprint, is_valid)` for the GPG/SSH signature on `hash`.
    ///
    /// Returns `(None, false)` when the commit carries no signature.
    /// Shells out to `git verify-commit --raw` for actual cryptographic
    /// verification so the system GPG/SSH key ring is consulted.
    pub fn commit_signature_status(&self, hash: &str) -> Result<(Option<String>, bool)> {
        let oid =
            git2::Oid::from_str(hash).map_err(|_| StorageError::InvalidRef(hash.to_owned()))?;

        // Fast check: does the commit even have a signature header?
        let has_sig = self
            .inner
            .extract_signature(&oid, Some("gpgsig"))
            .or_else(|_| self.inner.extract_signature(&oid, Some("gpgsig-sha256")))
            .is_ok();
        if !has_sig {
            return Ok((None, false));
        }

        let workdir = self.inner.workdir().ok_or(StorageError::NoWorkdir)?;

        let output = std::process::Command::new("git")
            .args(["verify-commit", "--raw", hash])
            .current_dir(workdir)
            .output()?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut fingerprint: Option<String> = None;
        let mut good_sig = false;

        for line in stderr.lines() {
            if !line.starts_with("[GNUPG:]") {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 3 {
                continue;
            }
            match parts[1] {
                "VALIDSIG" => fingerprint = Some(parts[2].to_owned()),
                "GOODSIG" => good_sig = true,
                "BADSIG" => good_sig = false,
                _ => {}
            }
        }

        Ok((fingerprint, good_sig))
    }

    /// Fetch updates from `url` into this repository's object database.
    ///
    /// Used to ensure a cloned external repo has the latest objects before
    /// resolving a versioned reference. Failures are non-fatal; the caller
    /// should log a warning and proceed with whatever is already cached.
    pub fn fetch_remote(&self, url: &str) -> Result<()> {
        let mut remote = self
            .inner
            .find_remote("origin")
            .or_else(|_| self.inner.remote_anonymous(url))?;
        remote.fetch(&[] as &[&str], None, None)?;
        Ok(())
    }

    /// Write a file and commit it in a single call.
    pub fn write_and_commit(
        &self,
        rel_path: impl AsRef<Path>,
        content: impl AsRef<[u8]>,
        message: &str,
        author: Option<&CommitAuthor>,
    ) -> Result<git2::Oid> {
        let rel_path = rel_path.as_ref();
        self.write_file(rel_path, content)?;
        self.commit(&[rel_path], message, author)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Initialise a bare-minimum git repo in a temp dir (no commits yet).
    fn init_repo() -> (TempDir, CodeRepository) {
        let dir = TempDir::new().unwrap();
        let inner = GitRepo::init(dir.path()).unwrap();
        // git requires at least a user.email and user.name for commits.
        let mut cfg = inner.config().unwrap();
        cfg.set_str("user.name", "Test").unwrap();
        cfg.set_str("user.email", "test@example.com").unwrap();
        drop(cfg);
        let repo = CodeRepository {
            inner,
            source: RepoSource::Local(dir.path().to_owned()),
        };
        (dir, repo)
    }

    #[test]
    fn open_or_init_creates_then_reopens() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("store");

        let repo = CodeRepository::open_or_init(&path).unwrap();
        repo.write_and_commit("a.clj", b"(ns a)", "init", None)
            .unwrap();

        // Second call must open the existing repo, not re-init it.
        let reopened = CodeRepository::open_or_init(&path).unwrap();
        assert_eq!(reopened.files_at_rev("HEAD").unwrap().len(), 1);
    }

    #[test]
    fn write_file_creates_file_on_disk() {
        let (_dir, repo) = init_repo();
        repo.write_file("src/core.clj", b"(ns test)").unwrap();
        let abs = repo.workdir().unwrap().join("src/core.clj");
        assert_eq!(std::fs::read(abs).unwrap(), b"(ns test)");
    }

    #[test]
    fn write_and_commit_round_trips_content() {
        let (_dir, repo) = init_repo();
        let author = CommitAuthor {
            name: "Bot".into(),
            email: "bot@test".into(),
        };

        let oid = repo
            .write_and_commit(
                "fn/retry.clj",
                "(defn retry [])",
                "add retry",
                Some(&author),
            )
            .unwrap();

        // The commit should be reachable from HEAD.
        let head_oid = repo.inner.head().unwrap().peel_to_commit().unwrap().id();
        assert_eq!(oid, head_oid);

        // Content should round-trip through files_at_rev.
        let files = repo.files_at_rev("HEAD").unwrap();
        let file = files
            .iter()
            .find(|f| f.path.to_str() == Some("fn/retry.clj"))
            .unwrap();
        assert_eq!(file.content_str().unwrap(), "(defn retry [])");
    }

    #[test]
    fn commit_two_files_in_one_shot() {
        let (_dir, repo) = init_repo();

        repo.write_file("a.clj", b"(ns a)").unwrap();
        repo.write_file("b.clj", b"(ns b)").unwrap();
        repo.commit(&["a.clj", "b.clj"], "add a and b", None)
            .unwrap();

        let files = repo.files_at_rev("HEAD").unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn second_commit_becomes_child_of_first() {
        let (_dir, repo) = init_repo();

        repo.write_and_commit("a.clj", b"v1", "first", None)
            .unwrap();
        let oid2 = repo
            .write_and_commit("a.clj", b"v2", "second", None)
            .unwrap();

        let commit2 = repo.inner.find_commit(oid2).unwrap();
        assert_eq!(commit2.parent_count(), 1);
    }
}
