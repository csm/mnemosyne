use std::{
    io::{Read, Write},
    path::Path,
};

use mnemosyne_code_search::IndexedFunction;
use mnemosyne_code_storage::CodeRepository;
use serde_json;
use tracing::info;

use crate::{
    embedder::{embed_text, Embedder, DIMENSION},
    error::SemanticSearchError,
};

/// A single hit returned by [`SemanticIndex::search`].
#[derive(Debug, Clone)]
pub struct SemanticResult {
    /// Cosine similarity in [0, 1]; higher is more relevant.
    pub score: f32,
    pub function: IndexedFunction,
}

/// In-memory vector index backed by flat cosine similarity search.
///
/// Flat search is O(n) per query, which is adequate for thousands of
/// functions. The index persists to disk so it survives restarts without
/// re-embedding. HNSW (via `hnsw_rs`) is a straightforward upgrade path
/// when the index grows large enough to warrant it.
pub struct SemanticIndex {
    embedder: Embedder,
    /// Parallel vecs: entry i has embedding `vectors[i]` and metadata `entries[i]`.
    /// Embeddings are L2-normalised on insert so cosine similarity == dot product.
    vectors: Vec<Vec<f32>>,
    entries: Vec<IndexedFunction>,
}

impl SemanticIndex {
    pub fn new(embedder: Embedder) -> Self {
        Self {
            embedder,
            vectors: Vec::new(),
            entries: Vec::new(),
        }
    }

    /// Open a previously saved index from `dir`, or create an empty one.
    pub fn open_or_create(
        dir: impl AsRef<Path>,
        embedder: Embedder,
    ) -> Result<Self, SemanticSearchError> {
        let dir = dir.as_ref();
        let meta_path = dir.join("metadata.json");
        let vectors_path = dir.join("vectors.bin");

        if meta_path.exists() && vectors_path.exists() {
            info!(dir = %dir.display(), "loading semantic index from disk");
            return Self::load(dir, embedder);
        }

        std::fs::create_dir_all(dir)?;
        Ok(Self::new(embedder))
    }

    /// Embed and index a batch of functions.
    pub fn add_functions(&mut self, fns: &[IndexedFunction]) -> Result<(), SemanticSearchError> {
        if fns.is_empty() {
            return Ok(());
        }
        let texts: Vec<String> = fns.iter().map(embed_text).collect();
        let mut embeddings = self.embedder.embed(texts)?;
        for emb in &mut embeddings {
            normalize(emb);
        }
        self.vectors.extend(embeddings);
        self.entries.extend_from_slice(fns);
        Ok(())
    }

    /// Embed and index all Clojure files in a `CodeRepository` at HEAD.
    pub fn index_repository(
        &mut self,
        repo: &CodeRepository,
        repo_name: &str,
    ) -> Result<(), SemanticSearchError> {
        let files = repo
            .files_at_rev("HEAD")
            .map_err(|e| SemanticSearchError::Embed(e.to_string()))?;

        let fns: Vec<IndexedFunction> = files
            .iter()
            .filter(|f| {
                f.path
                    .extension()
                    .is_some_and(|e| e == "clj" || e == "cljc" || e == "cljs")
            })
            .filter_map(|f| {
                let content = f.content_str()?;
                Some(IndexedFunction {
                    repo: repo_name.to_owned(),
                    file_path: f.path.to_string_lossy().into_owned(),
                    name: f.path.file_stem()?.to_string_lossy().into_owned(),
                    docstring: None,
                    body: content.to_owned(),
                })
            })
            .collect();

        info!(repo = repo_name, count = fns.len(), "indexing repository");
        self.add_functions(&fns)
    }

    /// Return the `top_k` most similar functions for the given intent string.
    pub fn search(
        &self,
        intent: &str,
        top_k: usize,
    ) -> Result<Vec<SemanticResult>, SemanticSearchError> {
        if self.vectors.is_empty() {
            return Err(SemanticSearchError::EmptyIndex);
        }

        let mut query_vec = self.embedder.embed(vec![intent.to_owned()])?.remove(0);
        normalize(&mut query_vec);

        let mut scored: Vec<(f32, usize)> = self
            .vectors
            .iter()
            .enumerate()
            .map(|(i, v)| (dot(&query_vec, v), i))
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        Ok(scored
            .into_iter()
            .take(top_k)
            .map(|(score, i)| SemanticResult {
                score,
                function: self.entries[i].clone(),
            })
            .collect())
    }

    /// Number of indexed functions.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    // ── Persistence ──────────────────────────────────────────────────────────

    /// Write the index to `dir`. Creates the directory if needed.
    /// Two files are written:
    /// - `metadata.json`: serde-serialised `Vec<IndexedFunction>`
    /// - `vectors.bin`:   raw little-endian f32 arrays, DIMENSION floats each
    pub fn save(&self, dir: impl AsRef<Path>) -> Result<(), SemanticSearchError> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir)?;

        // Metadata
        let json = serde_json::to_vec(&self.entries)?;
        std::fs::write(dir.join("metadata.json"), &json)?;

        // Vectors
        let mut file = std::fs::File::create(dir.join("vectors.bin"))?;
        for vec in &self.vectors {
            for f in vec {
                file.write_all(&f.to_le_bytes())?;
            }
        }

        info!(dir = %dir.display(), entries = self.entries.len(), "saved semantic index");
        Ok(())
    }

    fn load(dir: &Path, embedder: Embedder) -> Result<Self, SemanticSearchError> {
        let entries: Vec<IndexedFunction> =
            serde_json::from_slice(&std::fs::read(dir.join("metadata.json"))?)?;

        let raw = std::fs::read(dir.join("vectors.bin"))?;
        let expected_bytes = entries.len() * DIMENSION * 4;
        if raw.len() != expected_bytes {
            return Err(SemanticSearchError::Embed(format!(
                "vectors.bin size mismatch: expected {expected_bytes}, got {}",
                raw.len()
            )));
        }

        let mut file = std::io::Cursor::new(raw);
        let mut vectors = Vec::with_capacity(entries.len());
        for _ in 0..entries.len() {
            let mut vec = vec![0f32; DIMENSION];
            for x in &mut vec {
                let mut buf = [0u8; 4];
                file.read_exact(&mut buf)?;
                *x = f32::from_le_bytes(buf);
            }
            vectors.push(vec);
        }

        info!(dir = %dir.display(), entries = entries.len(), "loaded semantic index");
        Ok(Self {
            embedder,
            vectors,
            entries,
        })
    }
}

// ── Math helpers ─────────────────────────────────────────────────────────────

fn normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-9 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}
