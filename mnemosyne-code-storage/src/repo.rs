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

/// Thin wrapper around a git repository with helpers for walking file trees.
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

    /// Return the local working directory path.
    pub fn workdir(&self) -> Option<&Path> {
        self.inner.workdir()
    }

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
}
