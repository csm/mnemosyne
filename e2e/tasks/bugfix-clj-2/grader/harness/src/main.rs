//! Grader for bugfix-clj-2.
//!
//! Loads the agent's (fixed-up) `project/src/**/*.clj` and
//! `project/test/happy_path_test.clj` plus the hidden regression suite into
//! a real `mnemosyne-execution-engine::ClojureRuntime` -- the same
//! interpreter `clojure_eval` uses -- and runs `clojure.test/run-tests`.
//! Structurally identical to bugfix-clj's grader (see
//! ../../../bugfix-clj/grader/harness/src/main.rs); only the file list and
//! task name differ.
use mnemosyne_execution_engine::ClojureRuntime;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const SOURCE_FILES: &[&str] = &["src/dates/epoch.clj", "src/schedule/weekly.clj"];

const VISIBLE_TEST_FILE: &str = "test/happy_path_test.clj";

fn arg(name: &str) -> Option<String> {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if a == name {
            return args.next();
        }
    }
    None
}

fn main() -> ExitCode {
    let task_dir = PathBuf::from(arg("--task-dir").expect("--task-dir required"));
    let out_path = PathBuf::from(arg("--out").expect("--out required"));
    // run.sh always bind-mounts the task's grader/ directory at /grader in
    // the grading container; golden/ and hidden_tests/ live there, not
    // wherever this binary happened to be compiled.
    let grader_dir = PathBuf::from(arg("--grader-dir").unwrap_or_else(|| "/grader".to_string()));

    let project_dir = task_dir.join("project");
    let mut result = serde_json::json!({"task": "bugfix-clj-2", "score": 0.0, "checks": {}});

    if !project_dir.is_dir() {
        result["checks"]["project_present"] = serde_json::json!({"pass": false});
        write_and_print(&out_path, &result);
        return ExitCode::SUCCESS;
    }

    let golden = grader_dir.join("golden").join("happy_path_test.clj");
    let candidate = project_dir.join(VISIBLE_TEST_FILE);
    let untouched = match (fs::read(&golden), fs::read(&candidate)) {
        (Ok(g), Ok(c)) => g == c,
        _ => false,
    };
    result["checks"]["visible_tests_untouched"] = serde_json::json!({"pass": untouched});
    if !untouched {
        write_and_print(&out_path, &result);
        return ExitCode::SUCCESS;
    }

    let mut rt = ClojureRuntime::new();
    rt.load_string("(ns user)").expect("ns user");
    let clojure_test_src =
        cljrs_builtins::builtins::CLOJURE_TEST_SOURCE.replacen("(ns clojure.test)", "", 1);
    rt.load_string(&clojure_test_src).expect("load clojure.test");

    for rel in SOURCE_FILES {
        if let Err(e) = load_file(&mut rt, &project_dir.join(rel)) {
            result["checks"]["load_error"] = serde_json::json!({"file": rel, "error": e});
            write_and_print(&out_path, &result);
            return ExitCode::SUCCESS;
        }
    }
    if let Err(e) = load_file(&mut rt, &candidate) {
        result["checks"]["load_error"] =
            serde_json::json!({"file": VISIBLE_TEST_FILE, "error": e});
        write_and_print(&out_path, &result);
        return ExitCode::SUCCESS;
    }
    let hidden = grader_dir.join("hidden_tests").join("regressions_test.clj");
    if let Err(e) = load_file(&mut rt, &hidden) {
        result["checks"]["load_error"] =
            serde_json::json!({"file": "hidden_tests/regressions_test.clj", "error": e});
        write_and_print(&out_path, &result);
        return ExitCode::SUCCESS;
    }

    match rt.eval("(run-tests)") {
        Ok(v) => {
            let counts = value_to_json(&v);
            let total = counts.get("test").and_then(|v| v.as_i64()).unwrap_or(0);
            let fail = counts.get("fail").and_then(|v| v.as_i64()).unwrap_or(0);
            let error = counts.get("error").and_then(|v| v.as_i64()).unwrap_or(0);
            let score = if total > 0 {
                (total - fail - error) as f64 / total as f64
            } else {
                0.0
            };
            result["score"] = serde_json::json!(score);
            result["checks"]["hidden_suite"] = counts;
        }
        Err(e) => {
            result["checks"]["run_tests_error"] = serde_json::json!(format!("{e}"));
        }
    }

    write_and_print(&out_path, &result);
    ExitCode::SUCCESS
}

fn load_file(rt: &mut ClojureRuntime, path: &Path) -> Result<(), String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    rt.load_string(&src)
        .map_err(|e| format!("eval {}: {e}", path.display()))
}

/// `(run-tests)` returns a Clojure map `{:test n :pass n :fail n :error n}`;
/// convert its keyword keys/int values to a plain JSON object.
fn value_to_json(v: &mnemosyne_execution_engine::ClojureValue) -> serde_json::Value {
    use mnemosyne_execution_engine::ClojureValue as CV;
    match v {
        CV::Map(pairs) => {
            let mut obj = serde_json::Map::new();
            for (k, val) in pairs {
                let key = match k {
                    CV::Keyword(s) | CV::String(s) | CV::Symbol(s) => s.clone(),
                    other => format!("{other}"),
                };
                obj.insert(key, value_to_json(val));
            }
            serde_json::Value::Object(obj)
        }
        CV::Int(n) => serde_json::json!(n),
        CV::Float(n) => serde_json::json!(n),
        CV::Bool(b) => serde_json::json!(b),
        CV::String(s) => serde_json::json!(s),
        CV::Nil => serde_json::Value::Null,
        other => serde_json::json!(format!("{other}")),
    }
}

fn write_and_print(out_path: &Path, result: &serde_json::Value) {
    let text = serde_json::to_string_pretty(result).unwrap();
    fs::write(out_path, &text).expect("write grade.json");
    println!("{text}");
}
