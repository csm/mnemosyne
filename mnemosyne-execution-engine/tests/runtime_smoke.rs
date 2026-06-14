//! End-to-end smoke tests: drive the clojurust interpreter through the
//! `RuntimeHandle` actor and confirm evaluation works after the 0.1.x bump.

use mnemosyne_execution_engine::{ClojureValue, RuntimeHandle};

#[tokio::test]
async fn evaluates_arithmetic() {
    let rt = RuntimeHandle::spawn_minimal();
    let val = rt.eval("(+ 1 2)").await.expect("eval ok");
    assert_eq!(val, ClojureValue::Int(3));
}

#[tokio::test]
async fn def_then_use_persists_across_calls() {
    let rt = RuntimeHandle::spawn_minimal();
    rt.eval("(def x 41)").await.expect("def ok");
    let val = rt.eval("(inc x)").await.expect("use ok");
    assert_eq!(val, ClojureValue::Int(42));

    // The defined var shows up in the namespace's bindings.
    let names = rt.binding_names().await.expect("binding_names ok");
    assert!(names.iter().any(|n| n == "x"), "expected x in {names:?}");
}

#[tokio::test]
async fn returns_structured_collections() {
    let rt = RuntimeHandle::spawn_minimal();
    let val = rt.eval("[1 2 3]").await.expect("eval ok");
    assert_eq!(
        val,
        ClojureValue::Vector(vec![
            ClojureValue::Int(1),
            ClojureValue::Int(2),
            ClojureValue::Int(3),
        ])
    );
}

#[tokio::test]
async fn eval_error_is_reported_not_panicked() {
    let rt = RuntimeHandle::spawn_minimal();
    let result = rt.eval("(this-symbol-does-not-exist)").await;
    assert!(result.is_err(), "expected an eval error, got {result:?}");
    // The actor thread must still be alive and able to serve the next request.
    let val = rt.eval("(+ 2 2)").await.expect("runtime still alive");
    assert_eq!(val, ClojureValue::Int(4));
}
