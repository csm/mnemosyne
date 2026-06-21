//! Native tool-calling round-trip: the engine should advertise tools, execute
//! a `tool_use` the model requests, feed the `tool_result` back into the
//! transcript, and return the model's final text — all via structured content
//! blocks rather than JSON smuggled through a text field.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use mnemosyne_code_search::CodeIndex;
use mnemosyne_execution_engine::RuntimeHandle;
use mnemosyne_inference_engine::{
    ContentBlock, InferenceEngine, LlmBackend, LlmRequest, LlmResponse, Result, ToolCall,
};

/// A backend that scripts two turns: first request a Clojure eval, then — once
/// the result comes back — return a final answer.
struct ScriptedBackend {
    calls: AtomicUsize,
    tools_advertised: Mutex<bool>,
    saw_tool_result: Mutex<bool>,
}

#[async_trait]
impl LlmBackend for ScriptedBackend {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
        let turn = self.calls.fetch_add(1, Ordering::SeqCst);
        if turn == 0 {
            *self.tools_advertised.lock().unwrap() = !request.tools.is_empty();
            return Ok(LlmResponse {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "call_1".into(),
                    name: "eval_clojure".into(),
                    input: serde_json::json!({ "source": "(+ 1 2)" }),
                }],
                model: "scripted".into(),
                input_tokens: 0,
                output_tokens: 0,
            });
        }

        // Second turn: the transcript must now carry the tool result.
        let saw = request.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolResult { .. }))
        });
        *self.saw_tool_result.lock().unwrap() = saw;
        Ok(LlmResponse {
            text: "the sum is 3".into(),
            tool_calls: vec![],
            model: "scripted".into(),
            input_tokens: 0,
            output_tokens: 0,
        })
    }
}

#[tokio::test]
async fn runs_tool_then_returns_final_text() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let index_dir = std::env::temp_dir().join(format!("mnem-tool-loop-{nanos}"));
    let index = CodeIndex::open_or_create(&index_dir).expect("index");

    let backend = Arc::new(ScriptedBackend {
        calls: AtomicUsize::new(0),
        tools_advertised: Mutex::new(false),
        saw_tool_result: Mutex::new(false),
    });
    let engine = InferenceEngine::new(backend.clone(), RuntimeHandle::spawn_minimal(), index);

    let answer = engine.run("add one and two").await.expect("run ok");

    assert_eq!(answer, "the sum is 3");
    assert_eq!(backend.calls.load(Ordering::SeqCst), 2, "two turns");
    assert!(
        *backend.tools_advertised.lock().unwrap(),
        "tools must be advertised to the model"
    );
    assert!(
        *backend.saw_tool_result.lock().unwrap(),
        "the second turn must see the tool_result in the transcript"
    );
}
