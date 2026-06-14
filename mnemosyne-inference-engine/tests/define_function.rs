//! `define_function` is the promotion path: a brand-new fact/function should be
//! appended to a repo file, committed, and indexed so a later `search_code`
//! finds it. Covered for both a bare `defn` and the recommended namespaced
//! (`(ns facts.*)`) grouping.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use mnemosyne_code_search::{CodeIndex, SearchQuery};
use mnemosyne_execution_engine::RuntimeHandle;
use mnemosyne_inference_engine::{
    InferenceEngine, LlmBackend, LlmRequest, LlmResponse, Result, ToolCall,
};

/// Scripts one `define_function` call (with a configurable target file and
/// source), then a final answer.
struct DefineBackend {
    calls: AtomicUsize,
    file: String,
    source: String,
}

#[async_trait]
impl LlmBackend for DefineBackend {
    async fn complete(&self, _request: LlmRequest) -> Result<LlmResponse> {
        let turn = self.calls.fetch_add(1, Ordering::SeqCst);
        if turn == 0 {
            return Ok(LlmResponse {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "c1".into(),
                    name: "define_function".into(),
                    input: serde_json::json!({
                        "repo": "facts",
                        "file": self.file,
                        "source": self.source,
                    }),
                }],
                model: "scripted".into(),
                input_tokens: 0,
                output_tokens: 0,
            });
        }
        Ok(LlmResponse {
            text: "remembered".into(),
            tool_calls: vec![],
            model: "scripted".into(),
            input_tokens: 0,
            output_tokens: 0,
        })
    }
}

/// Run a `define_function` turn and return the repo workdir and index dir.
async fn run_define(tag: &str, file: &str, source: &str) -> (PathBuf, PathBuf) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let work = std::env::temp_dir().join(format!("mnem-{tag}-{nanos}"));
    std::fs::create_dir_all(&work).unwrap();
    git2::Repository::init(&work).expect("git init");

    let index_dir = std::env::temp_dir().join(format!("mnem-{tag}-idx-{nanos}"));
    let index = CodeIndex::open_or_create(&index_dir).expect("index");

    let backend = Arc::new(DefineBackend {
        calls: AtomicUsize::new(0),
        file: file.to_owned(),
        source: source.to_owned(),
    });
    let engine = InferenceEngine::new(backend, RuntimeHandle::spawn_minimal(), index)
        .with_repo("facts", &work);

    let answer = engine.run("remember this").await.expect("run ok");
    assert_eq!(answer, "remembered");

    (work, index_dir)
}

/// Assert `name` is committed to HEAD and discoverable via `query`.
fn assert_committed_and_indexed(work: &PathBuf, index_dir: &PathBuf, query: &str, name: &str) {
    let repo = git2::Repository::open(work).unwrap();
    assert!(repo.head().is_ok(), "a commit should exist on HEAD");

    let reopened = CodeIndex::open_or_create(index_dir).expect("reopen index");
    let hits = reopened
        .search(&SearchQuery::new(query).with_limit(5))
        .expect("search ok");
    assert!(
        hits.iter().any(|h| h.function.name == name),
        "promoted fact {name:?} should be searchable, got {:?}",
        hits.iter().map(|h| &h.function.name).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn promotes_bare_defn_into_committed_and_indexed_store() {
    let (work, index_dir) = run_define(
        "def",
        "facts.clj",
        "(defn fact-answer\n  \"The answer to everything.\"\n  [] 42)",
    )
    .await;

    let content = std::fs::read_to_string(work.join("facts.clj")).expect("file written");
    assert!(content.contains("fact-answer"), "got: {content}");

    assert_committed_and_indexed(&work, &index_dir, "answer", "fact-answer");
}

#[tokio::test]
async fn promotes_namespaced_fact_into_committed_and_indexed_store() {
    // The recommended shape: group facts under a namespace and define each as a
    // plain zero-arg fn. define_function should skip the (ns ...) form and index
    // the function by its short name.
    let (work, index_dir) = run_define(
        "ns",
        "facts/system_info.clj",
        "(ns facts.system-info)\n\n(defn home-dir [] \"/home/user\")",
    )
    .await;

    let content =
        std::fs::read_to_string(work.join("facts/system_info.clj")).expect("file written");
    assert!(content.contains("(ns facts.system-info)"), "got: {content}");
    assert!(content.contains("home-dir"), "got: {content}");

    assert_committed_and_indexed(&work, &index_dir, "home-dir", "home-dir");
}
