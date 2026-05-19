use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::edn::ToEdn;
use crate::episode::{Episode, EpisodeKind, SessionId};
use crate::error::MemoryError;

type Result<T> = std::result::Result<T, MemoryError>;

// ── Session index ─────────────────────────────────────────────────────────────

/// Flat list of sessions stored under a base directory, newest last.
#[derive(Debug, Default, Serialize, Deserialize)]
struct SessionIndex {
    entries: Vec<SessionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionEntry {
    id: SessionId,
    started_at: String,
}

// ── MemoryStore ───────────────────────────────────────────────────────────────

/// Append-only episodic log for a single agent session.
///
/// Episodes are written as newline-delimited JSON (one object per line) to
/// `{base_dir}/{session_id}.jsonl`. The full session is also readable as EDN
/// via `export_edn`.
pub struct MemoryStore {
    session_id: SessionId,
    #[allow(dead_code)]
    base_dir: PathBuf,
    seq: u64,
    cache: Vec<Episode>,
    writer: BufWriter<File>,
}

impl MemoryStore {
    /// Create a new session under `base_dir`.
    pub fn create(base_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(base_dir)?;
        let id = SessionId::new();
        let log_path = base_dir.join(format!("{id}.jsonl"));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;

        let mut index = load_index(base_dir).unwrap_or_default();
        index.entries.push(SessionEntry {
            id: id.clone(),
            started_at: Utc::now().to_rfc3339(),
        });
        save_index(base_dir, &index)?;

        Ok(Self {
            session_id: id,
            base_dir: base_dir.to_path_buf(),
            seq: 0,
            cache: Vec::new(),
            writer: BufWriter::new(file),
        })
    }

    /// Open an existing session by ID, replaying its log into memory.
    pub fn open(base_dir: &Path, id: &SessionId) -> Result<Self> {
        let log_path = base_dir.join(format!("{id}.jsonl"));
        if !log_path.exists() {
            return Err(MemoryError::NotFound(id.to_string()));
        }

        let mut cache = Vec::new();
        let read_file = File::open(&log_path)?;
        for line in BufReader::new(read_file).lines() {
            let line = line?;
            if !line.trim().is_empty() {
                let ep: Episode = serde_json::from_str(&line)?;
                cache.push(ep);
            }
        }

        let seq = cache.len() as u64;
        let append_file = OpenOptions::new().append(true).open(&log_path)?;

        Ok(Self {
            session_id: id.clone(),
            base_dir: base_dir.to_path_buf(),
            seq,
            cache,
            writer: BufWriter::new(append_file),
        })
    }

    /// Open the most recently created session under `base_dir`, if any exists.
    pub fn open_latest(base_dir: &Path) -> Result<Option<Self>> {
        let index = load_index(base_dir).unwrap_or_default();
        match index.entries.last() {
            None => Ok(None),
            Some(entry) => Self::open(base_dir, &entry.id).map(Some),
        }
    }

    /// Append one episode to the log and return a reference to it.
    pub fn log(&mut self, kind: EpisodeKind) -> Result<&Episode> {
        let episode = Episode {
            seq: self.seq,
            session_id: self.session_id.clone(),
            timestamp: Utc::now(),
            kind,
        };

        let line = serde_json::to_string(&episode)?;
        writeln!(self.writer, "{line}")?;
        self.writer.flush()?;

        self.seq += 1;
        self.cache.push(episode);
        Ok(self.cache.last().unwrap())
    }

    /// Return the last `n` episodes (or all of them if fewer than `n` exist).
    pub fn recent(&self, n: usize) -> &[Episode] {
        let start = self.cache.len().saturating_sub(n);
        &self.cache[start..]
    }

    /// Return every episode in this session.
    pub fn all(&self) -> &[Episode] {
        &self.cache
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Render the full session as newline-delimited EDN maps.
    pub fn export_edn(&self) -> String {
        self.cache
            .iter()
            .map(|ep| ep.to_edn())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ── Index helpers ─────────────────────────────────────────────────────────────

fn index_path(base_dir: &Path) -> PathBuf {
    base_dir.join("sessions.json")
}

fn load_index(base_dir: &Path) -> Result<SessionIndex> {
    let path = index_path(base_dir);
    if !path.exists() {
        return Ok(SessionIndex::default());
    }
    let s = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&s)?)
}

fn save_index(base_dir: &Path, index: &SessionIndex) -> Result<()> {
    std::fs::write(index_path(base_dir), serde_json::to_string_pretty(index)?)?;
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn create_log_and_read_back() {
        let dir = TempDir::new().unwrap();
        let session_id = {
            let mut store = MemoryStore::create(dir.path()).unwrap();
            store
                .log(EpisodeKind::UserMessage {
                    content: "hello".into(),
                })
                .unwrap();
            store
                .log(EpisodeKind::AssistantReply {
                    content: "world".into(),
                })
                .unwrap();
            assert_eq!(store.all().len(), 2);
            store.session_id().clone()
        };

        let store = MemoryStore::open(dir.path(), &session_id).unwrap();
        assert_eq!(store.all().len(), 2);
        assert_eq!(store.recent(1).len(), 1);

        if let EpisodeKind::UserMessage { content } = &store.all()[0].kind {
            assert_eq!(content, "hello");
        } else {
            panic!("unexpected kind");
        }
    }

    #[test]
    fn recent_clamps_to_available() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::create(dir.path()).unwrap();
        for i in 0..5u64 {
            store
                .log(EpisodeKind::Note {
                    content: i.to_string(),
                })
                .unwrap();
        }
        assert_eq!(store.recent(3).len(), 3);
        assert_eq!(store.recent(10).len(), 5);
        assert_eq!(store.recent(0).len(), 0);
    }

    #[test]
    fn open_latest_returns_newest() {
        let dir = TempDir::new().unwrap();

        let id1 = {
            let mut s = MemoryStore::create(dir.path()).unwrap();
            s.log(EpisodeKind::Note {
                content: "session 1".into(),
            })
            .unwrap();
            s.session_id().clone()
        };

        let id2 = {
            let mut s = MemoryStore::create(dir.path()).unwrap();
            s.log(EpisodeKind::Note {
                content: "session 2".into(),
            })
            .unwrap();
            s.session_id().clone()
        };

        assert_ne!(id1, id2);
        let latest = MemoryStore::open_latest(dir.path()).unwrap().unwrap();
        assert_eq!(latest.session_id(), &id2);
    }

    #[test]
    fn open_latest_empty_dir_returns_none() {
        let dir = TempDir::new().unwrap();
        let result = MemoryStore::open_latest(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn edn_export_contains_all_episodes() {
        let dir = TempDir::new().unwrap();
        let mut store = MemoryStore::create(dir.path()).unwrap();
        store
            .log(EpisodeKind::UserMessage {
                content: "find retry functions".into(),
            })
            .unwrap();
        store
            .log(EpisodeKind::ToolCall {
                tool: "search_code".into(),
                input: serde_json::json!({"query": "retry", "limit": 5}),
            })
            .unwrap();

        let edn = store.export_edn();
        let lines: Vec<&str> = edn.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains(":user-message"));
        assert!(lines[1].contains(":tool-call"));
        assert!(lines[0].contains(":seq 0"));
        assert!(lines[1].contains(":seq 1"));
    }

    #[test]
    fn seq_increments_across_reopen() {
        let dir = TempDir::new().unwrap();
        let session_id = {
            let mut s = MemoryStore::create(dir.path()).unwrap();
            s.log(EpisodeKind::Note {
                content: "first".into(),
            })
            .unwrap();
            s.session_id().clone()
        };

        let mut store = MemoryStore::open(dir.path(), &session_id).unwrap();
        store
            .log(EpisodeKind::Note {
                content: "second".into(),
            })
            .unwrap();

        assert_eq!(store.all()[0].seq, 0);
        assert_eq!(store.all()[1].seq, 1);
    }
}
