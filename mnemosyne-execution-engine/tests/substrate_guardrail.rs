//! The async IO / networking substrate must be gated by `IoPolicy`: absent by
//! default, present only when explicitly allowed.

use std::time::{SystemTime, UNIX_EPOCH};

use mnemosyne_execution_engine::{ClojureValue, IoPolicy, RuntimeHandle};

#[tokio::test]
async fn io_symbols_absent_under_default_policy() {
    let rt = RuntimeHandle::spawn_minimal(); // deny-all
    // The async file-IO namespace is not loaded, so the symbol does not resolve.
    let result = rt.eval("clojure.rust.io.async/slurp").await;
    assert!(
        result.is_err(),
        "io substrate must not be registered under deny-all, got {result:?}"
    );
}

#[tokio::test]
async fn substrate_present_when_allowed() {
    let rt = RuntimeHandle::spawn_minimal_with_policy(IoPolicy::allow_all());

    // core.async is the prerequisite and must be loaded.
    let async_ns = rt
        .eval("(some? (find-ns 'clojure.core.async))")
        .await
        .expect("eval ok");
    assert_eq!(
        async_ns,
        mnemosyne_execution_engine::ClojureValue::Bool(true),
        "core.async should be loaded under allow-all"
    );

    // The async file-IO symbol now resolves to a value (a function).
    rt.eval("clojure.rust.io.async/slurp")
        .await
        .expect("io symbol should resolve under allow-all");

    // Ordinary (synchronous) evaluation still works on the LocalSet-backed
    // job loop, not just symbol resolution.
    let sum = rt.eval("(+ 1 2)").await.expect("eval ok");
    assert_eq!(sum, mnemosyne_execution_engine::ClojureValue::Int(3));
}

#[tokio::test]
async fn async_file_io_round_trip() {
    // Exercises the cooperative eval_async path end to end: file IO delivered
    // over core.async channels would deadlock under a blocking take, but with
    // eval_async the `await` yields on the LocalSet so producers can run.
    let rt = RuntimeHandle::spawn_minimal_with_policy(IoPolicy::allow_all());

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("mnem-io-{nanos}.txt"));
    let path_str = path.to_string_lossy().into_owned();

    rt.eval(format!(
        "(await (clojure.core.async/take! \
           (clojure.rust.io.async/spit \"{path_str}\" \"hello async\")))"
    ))
    .await
    .expect("spit ok");

    let read = rt
        .eval(format!(
            "(await (clojure.core.async/take! \
               (clojure.rust.io.async/slurp \"{path_str}\")))"
        ))
        .await
        .expect("slurp ok");

    assert_eq!(read, ClojureValue::String("hello async".into()));
    let _ = std::fs::remove_file(&path);
}
