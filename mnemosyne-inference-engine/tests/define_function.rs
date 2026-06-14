//! `define_function` is the promotion path: a brand-new fact/function should be
//! appended to a repo file, committed, and indexed so a later `search_code`
//! finds it.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use mnemosyne_code_search::{CodeIndex, SearchQuery};
use mnemosyne_execution_engine::RuntimeHandle;
use mnemosyne_inference_engine::{
    InferenceEngine, LlmBackend, LlmRequest, LlmResponse, Result, ToolCall,
};

/// Scripts one `define_function` call, then a final answer.
struct DefineBackend {
    calls: std::sync::atomic::AtomicUsize,
}

#[async_trait]
impl LlmBackend for DefineBackend {
    async fn complete(&self, _request: LlmRequest) -> Result<LlmResponse> {
        use std::sync::atomic::Ordering;
        let turn = self.calls.fetch_add(1, Ordering::SeqCst);
        if turn == 0 {
            return Ok(LlmResponse {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "c1".into(),
                    name: "define_function".into(),
                    input: serde_json::json!({
                        "repo": "facts",
                        "file": "facts.clj",
                        "source": "(defn fact-answer\n  \"The answer to everything.\"\n  [] 42)"
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

#[tokio::test]
async fn promotes_new_fact_into_committed_and_indexed_store() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let work = std::env::temp_dir().join(format!("mnem-def-{nanos}"));
    std::fs::create_dir_all(&work).unwrap();
    git2::Repository::init(&work).expect("git init");

    let index_dir = std::env::temp_dir().join(format!("mnem-def-idx-{nanos}"));
    let index = CodeIndex::open_or_create(&index_dir).expect("index");

    let backend = Arc::new(DefineBackend {
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let engine = InferenceEngine::new(backend, RuntimeHandle::spawn_minimal(), index)
        .with_repo("facts", &work);

    let answer = engine.run("remember the answer").await.expect("run ok");
    assert_eq!(answer, "remembered");

    // Persisted to the working tree.
    let content = std::fs::read_to_string(work.join("facts.clj")).expect("file written");
    assert!(
        content.contains("fact-answer"),
        "definition should be in the file: {content}"
    );

    // Committed to git.
    let repo = git2::Repository::open(&work).unwrap();
    assert!(repo.head().is_ok(), "a commit should exist on HEAD");

    // Indexed and therefore discoverable via search.
    let reopened = CodeIndex::open_or_create(&index_dir).expect("reopen index");
    let hits = reopened
        .search(&SearchQuery::new("answer").with_limit(5))
        .expect("search ok");
    assert!(
        hits.iter().any(|h| h.function.name == "fact-answer"),
        "promoted fact should be searchable, got {:?}",
        hits.iter().map(|h| &h.function.name).collect::<Vec<_>>()
    );
}
