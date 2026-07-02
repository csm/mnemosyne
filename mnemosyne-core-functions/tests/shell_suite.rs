//! End-to-end tests for the `mnemosyne.shell` utilities: each test boots a
//! runtime with the file-IO substrate, loads the embedded shell namespace,
//! and drives real files through channel pipelines.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use mnemosyne_core_functions::embedded;
use mnemosyne_execution_engine::{ClojureValue, IoPolicy, RuntimeHandle};

/// A runtime with file IO granted and `mnemosyne.shell` loaded.
async fn shell_runtime() -> RuntimeHandle {
    let rt = RuntimeHandle::spawn_minimal_with_policy(IoPolicy {
        async_enabled: true,
        file_io: true,
        network: false,
    });
    rt.eval(embedded::SHELL_CLJ)
        .await
        .expect("mnemosyne.shell should load");
    rt
}

/// A unique scratch directory for one test.
fn scratch_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("mnem-shell-{label}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn strings(items: &[&str]) -> ClojureValue {
    ClojureValue::Vector(
        items
            .iter()
            .map(|s| ClojureValue::String((*s).to_string()))
            .collect(),
    )
}

#[tokio::test]
async fn cat_streams_lines_in_order() {
    let dir = scratch_dir("cat");
    let path = dir.join("a.txt");
    fs::write(&path, "alpha\nbeta\ngamma\n").unwrap();
    let rt = shell_runtime().await;

    let v = rt
        .eval(format!(
            "(await (clojure.core.async/take! \
               (mnemosyne.shell/collect (mnemosyne.shell/cat \"{}\"))))",
            path.display()
        ))
        .await
        .expect("cat pipeline ok");
    assert_eq!(v, strings(&["alpha", "beta", "gamma"]));
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn cat_applies_transducer_per_line() {
    let dir = scratch_dir("cat-xf");
    let path = dir.join("a.txt");
    fs::write(&path, "one\ntwo\nthree\n").unwrap();
    let rt = shell_runtime().await;

    // Optional transducer argument: uppercase every line, drop short ones.
    let v = rt
        .eval(format!(
            "(await (clojure.core.async/take! \
               (mnemosyne.shell/collect \
                 (mnemosyne.shell/cat \"{}\" \
                   (comp (filter (fn [l] (> (count l) 3))) \
                         (map upper-case))))))",
            path.display()
        ))
        .await
        .expect("cat+xf pipeline ok");
    assert_eq!(v, strings(&["THREE"]));
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn head_terminates_the_stream_early() {
    let dir = scratch_dir("head");
    let path = dir.join("many.txt");
    let body: String = (0..500).map(|i| format!("line-{i}\n")).collect();
    fs::write(&path, body).unwrap();
    let rt = shell_runtime().await;

    let v = rt
        .eval(format!(
            "(await (clojure.core.async/take! \
               (mnemosyne.shell/collect \
                 (mnemosyne.shell/head 2 (mnemosyne.shell/cat \"{}\")))))",
            path.display()
        ))
        .await
        .expect("head pipeline ok");
    assert_eq!(v, strings(&["line-0", "line-1"]));
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn tail_yields_the_last_lines() {
    let dir = scratch_dir("tail");
    let path = dir.join("a.txt");
    fs::write(&path, "one\ntwo\nthree\nfour\n").unwrap();
    let rt = shell_runtime().await;

    let v = rt
        .eval(format!(
            "(await (clojure.core.async/take! \
               (mnemosyne.shell/collect \
                 (mnemosyne.shell/tail 2 (mnemosyne.shell/cat \"{}\")))))",
            path.display()
        ))
        .await
        .expect("tail pipeline ok");
    assert_eq!(v, strings(&["three", "four"]));
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn ls_lists_entries_sorted_with_types() {
    let dir = scratch_dir("ls");
    fs::write(dir.join("b.txt"), "b").unwrap();
    fs::write(dir.join("a.txt"), "a").unwrap();
    fs::create_dir(dir.join("sub")).unwrap();
    let rt = shell_runtime().await;

    let names = rt
        .eval(format!(
            "(vec (map (fn [e] [(:name e) (:type e)]) \
               (await (clojure.core.async/take! \
                 (mnemosyne.shell/collect (mnemosyne.shell/ls \"{}\"))))))",
            dir.display()
        ))
        .await
        .expect("ls pipeline ok");
    let pair = |n: &str, t: &str| {
        ClojureValue::Vector(vec![
            ClojureValue::String(n.into()),
            ClojureValue::Keyword(t.into()),
        ])
    };
    assert_eq!(
        names,
        ClojureValue::Vector(vec![
            pair("a.txt", "file"),
            pair("b.txt", "file"),
            pair("sub", "dir"),
        ])
    );
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn find_filters_by_glob_and_type() {
    let dir = scratch_dir("find");
    fs::write(dir.join("a.clj"), "(ns a)").unwrap();
    fs::write(dir.join("b.txt"), "b").unwrap();
    fs::create_dir(dir.join("sub")).unwrap();
    fs::write(dir.join("sub").join("c.clj"), "(ns c)").unwrap();
    let rt = shell_runtime().await;

    let v = rt
        .eval(format!(
            "(vec (map :name \
               (await (clojure.core.async/take! \
                 (mnemosyne.shell/collect \
                   (mnemosyne.shell/find \"{}\" {{:name \"*.clj\" :type :file}}))))))",
            dir.display()
        ))
        .await
        .expect("find pipeline ok");
    assert_eq!(v, strings(&["a.clj", "c.clj"]));
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn find_respects_max_depth() {
    let dir = scratch_dir("find-depth");
    fs::write(dir.join("top.txt"), "t").unwrap();
    fs::create_dir(dir.join("sub")).unwrap();
    fs::write(dir.join("sub").join("deep.txt"), "d").unwrap();
    let rt = shell_runtime().await;

    let v = rt
        .eval(format!(
            "(vec (map :name \
               (await (clojure.core.async/take! \
                 (mnemosyne.shell/collect \
                   (mnemosyne.shell/find \"{}\" {{:type :file :max-depth 1}}))))))",
            dir.display()
        ))
        .await
        .expect("find pipeline ok");
    assert_eq!(v, strings(&["top.txt"]));
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn grep_reports_line_numbers_and_text() {
    let dir = scratch_dir("grep");
    let path = dir.join("a.txt");
    fs::write(&path, "hello world\ngoodbye\nhello again\n").unwrap();
    let rt = shell_runtime().await;

    let v = rt
        .eval(format!(
            "(vec (map (fn [m] [(:line m) (:text m)]) \
               (await (clojure.core.async/take! \
                 (mnemosyne.shell/collect \
                   (mnemosyne.shell/grep #\"hello\" \"{}\"))))))",
            path.display()
        ))
        .await
        .expect("grep pipeline ok");
    let hit = |n: i64, t: &str| {
        ClojureValue::Vector(vec![ClojureValue::Int(n), ClojureValue::String(t.into())])
    };
    assert_eq!(
        v,
        ClojureValue::Vector(vec![hit(1, "hello world"), hit(3, "hello again")])
    );
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn find_pipes_into_grep() {
    let dir = scratch_dir("find-grep");
    fs::write(dir.join("a.clj"), "(defn f [x] x)\n").unwrap();
    fs::create_dir(dir.join("sub")).unwrap();
    fs::write(dir.join("sub").join("b.clj"), ";; comment\n(defn g [] 1)\n").unwrap();
    fs::write(dir.join("notes.txt"), "defn is a macro\n").unwrap();
    let rt = shell_runtime().await;

    // Directory entries stream straight from find into grep; the .txt file is
    // excluded by the glob and the directories are skipped by grep itself.
    let v = rt
        .eval(format!(
            "(vec (map (fn [m] [(re-find #\"[^/]+$\" (:path m)) (:line m)]) \
               (await (clojure.core.async/take! \
                 (mnemosyne.shell/collect \
                   (mnemosyne.shell/grep #\"defn\" \
                     (mnemosyne.shell/find \"{}\" {{:name \"*.clj\"}})))))))",
            dir.display()
        ))
        .await
        .expect("find|grep pipeline ok");
    let hit = |n: &str, line: i64| {
        ClojureValue::Vector(vec![
            ClojureValue::String(n.into()),
            ClojureValue::Int(line),
        ])
    };
    assert_eq!(
        v,
        ClojureValue::Vector(vec![hit("a.clj", 1), hit("b.clj", 2)])
    );
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn wc_l_counts_lines() {
    let dir = scratch_dir("wc");
    let path = dir.join("a.txt");
    fs::write(&path, "1\n2\n3\n4\n5\n").unwrap();
    let rt = shell_runtime().await;

    let v = rt
        .eval(format!(
            "(await (clojure.core.async/take! \
               (mnemosyne.shell/wc-l (mnemosyne.shell/cat \"{}\"))))",
            path.display()
        ))
        .await
        .expect("wc-l ok");
    assert_eq!(v, ClojureValue::Int(5));
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn write_lines_round_trips_through_a_pipeline() {
    let dir = scratch_dir("write");
    let src = dir.join("src.txt");
    let dst = dir.join("dst.txt");
    fs::write(&src, "keep one\ndrop\nkeep two\n").unwrap();
    let rt = shell_runtime().await;

    let bytes = rt
        .eval(format!(
            "(await (clojure.core.async/take! \
               (mnemosyne.shell/write-lines \"{}\" \
                 (mnemosyne.shell/grep #\"keep\" \"{}\" (map :text)))))",
            dst.display(),
            src.display()
        ))
        .await
        .expect("write-lines ok");
    assert_eq!(
        bytes,
        ClojureValue::Int("keep one\nkeep two\n".len() as i64)
    );
    assert_eq!(fs::read_to_string(&dst).unwrap(), "keep one\nkeep two\n");
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn cp_copies_a_file() {
    let dir = scratch_dir("cp");
    let src = dir.join("src.bin");
    let dst = dir.join("dst.bin");
    fs::write(&src, b"raw \x00 bytes").unwrap();
    let rt = shell_runtime().await;

    rt.eval(format!(
        "(await (clojure.core.async/take! \
           (mnemosyne.shell/cp \"{}\" \"{}\")))",
        src.display(),
        dst.display()
    ))
    .await
    .expect("cp ok");
    assert_eq!(fs::read(&dst).unwrap(), fs::read(&src).unwrap());
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn stat_and_exists_report_metadata() {
    let dir = scratch_dir("stat");
    let path = dir.join("a.txt");
    fs::write(&path, "12345").unwrap();
    let rt = shell_runtime().await;

    let v = rt
        .eval(format!(
            "(let [s (await (clojure.core.async/take! (mnemosyne.shell/stat \"{}\")))] \
               [(:type s) (:size s) \
                (await (mnemosyne.shell/exists? \"{}\")) \
                (await (mnemosyne.shell/exists? \"{}/missing\"))])",
            path.display(),
            path.display(),
            dir.display()
        ))
        .await
        .expect("stat ok");
    assert_eq!(
        v,
        ClojureValue::Vector(vec![
            ClojureValue::Keyword("file".into()),
            ClojureValue::Int(5),
            ClojureValue::Bool(true),
            ClojureValue::Bool(false),
        ])
    );
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn io_errors_are_delivered_in_band() {
    let dir = scratch_dir("err");
    let rt = shell_runtime().await;

    let v = rt
        .eval(format!(
            "(clojure.rust.io.async/error? \
               (first (await (clojure.core.async/take! \
                 (mnemosyne.shell/collect \
                   (mnemosyne.shell/cat \"{}/does-not-exist\"))))))",
            dir.display()
        ))
        .await
        .expect("error pipeline ok");
    assert_eq!(v, ClojureValue::Bool(true));
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn shell_namespace_absent_without_file_io() {
    // The native prerequisites are gated by IoPolicy, so loading the shell
    // namespace on a deny-all runtime must fail rather than half-define it.
    let rt = RuntimeHandle::spawn_minimal();
    let result = rt.eval(embedded::SHELL_CLJ).await;
    assert!(
        result.is_err(),
        "shell namespace must not load under deny-all, got {result:?}"
    );
}
