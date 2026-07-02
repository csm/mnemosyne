//! Native filesystem builtins for `mnemosyne.shell.native`.
//!
//! The cljrs-io substrate covers reading and writing file *contents*
//! (`line-chan`, `slurp`, `spit`, …) but has no directory or metadata
//! primitives, so shell-style utilities like `ls` and `find` cannot be
//! expressed on top of it. This module fills that gap with three builtins,
//! registered only when [`IoPolicy::file_io`](crate::IoPolicy) is granted:
//!
//! - `(dir-chan path)` / `(dir-chan path cap)` — a streaming channel of the
//!   directory's entries, sorted by name, closed after the last entry.
//! - `(walk-chan path)` / `(walk-chan path cap)` — a streaming channel of the
//!   root and every entry beneath it, depth-first with children in name
//!   order. Symlinks are reported but never followed, so cycles are
//!   impossible.
//! - `(stat path)` — a promise channel delivering the entry's metadata.
//!
//! Every entry is a map `{:path :name :type}` with `:type` one of `:file`,
//! `:dir`, `:symlink`, or `:other`; `walk-chan` entries additionally carry
//! `:depth` (root = 0) and `stat` adds `:size`, `:modified` (epoch millis,
//! when available), and `:readonly`.
//!
//! Delivery follows the cljrs-io conventions: results ride `core.async`
//! channels whose small buffer provides backpressure, and failures are
//! delivered in-band as error values (test with
//! `clojure.rust.io.async/error?`). A failure to list one subdirectory during
//! a walk emits an error value and continues, like `find` reporting
//! "Permission denied" and moving on.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use cljrs_async::channel::{chan_deliver as deliver, chan_put as put, chan_ref, make_chan};
use cljrs_async::spawn_future;
use cljrs_eval::GlobalEnv;
use cljrs_gc::GcPtr;
use cljrs_value::keyword::Keyword;
use cljrs_value::{Arity, ExceptionInfo, MapValue, NativeFn, Value, ValueError, ValueResult};

/// Namespace the builtins are interned into.
pub const NS: &str = "mnemosyne.shell.native";

/// Default backpressure buffer for streaming channels.
const DEFAULT_STREAM_CAP: usize = 8;

type Builtin = fn(&[Value]) -> ValueResult<Value>;

/// Register the shell filesystem builtins into `globals`.
pub fn register(globals: &Arc<GlobalEnv>) {
    let fns: Vec<(&str, Arity, Builtin)> = vec![
        ("dir-chan", Arity::Variadic { min: 1 }, builtin_dir_chan),
        ("walk-chan", Arity::Variadic { min: 1 }, builtin_walk_chan),
        ("stat", Arity::Fixed(1), builtin_stat),
    ];
    for (name, arity, func) in fns {
        let nf = NativeFn::new(name, arity, func);
        globals.intern(NS, Arc::from(name), Value::NativeFunction(GcPtr::new(nf)));
    }
}

// ── Value helpers ─────────────────────────────────────────────────────────────

fn kw(name: &str) -> Value {
    Value::keyword(Keyword::simple(name))
}

/// Build an in-band error value to put on a channel when an I/O step fails.
fn io_error(msg: impl Into<String>) -> Value {
    let msg = msg.into();
    Value::Error(GcPtr::new(ExceptionInfo::new(
        ValueError::Other(msg.clone()),
        msg,
        None,
        None,
    )))
}

fn type_keyword(ft: std::fs::FileType) -> Value {
    if ft.is_symlink() {
        kw("symlink")
    } else if ft.is_dir() {
        kw("dir")
    } else if ft.is_file() {
        kw("file")
    } else {
        kw("other")
    }
}

fn entry_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// `{:path … :name … :type …}`, plus `:depth` when `depth` is given.
fn entry_value(path: &Path, ft: std::fs::FileType, depth: Option<i64>) -> Value {
    let mut pairs = vec![
        (
            kw("path"),
            Value::string(path.to_string_lossy().into_owned()),
        ),
        (kw("name"), Value::string(entry_name(path))),
        (kw("type"), type_keyword(ft)),
    ];
    if let Some(d) = depth {
        pairs.push((kw("depth"), Value::Long(d)));
    }
    Value::Map(MapValue::from_pairs(pairs))
}

// ── Argument parsing ──────────────────────────────────────────────────────────

fn str_arg(args: &[Value], idx: usize, expected: &'static str) -> ValueResult<String> {
    match args.get(idx) {
        Some(Value::Str(s)) => Ok(s.get().clone()),
        other => Err(ValueError::WrongType {
            expected,
            got: other.map(|v| v.type_name().to_string()).unwrap_or_default(),
        }),
    }
}

fn size_arg_or(args: &[Value], idx: usize, default: usize) -> ValueResult<usize> {
    match args.get(idx) {
        None | Some(Value::Nil) => Ok(default),
        Some(Value::Long(n)) if *n >= 0 => Ok((*n as usize).max(1)),
        Some(other) => Err(ValueError::WrongType {
            expected: "non-negative long",
            got: other.type_name().to_string(),
        }),
    }
}

// ── Directory listing ─────────────────────────────────────────────────────────

/// List `path`, returning entries sorted by name. Each element is
/// `(PathBuf, FileType)`; the file type comes from the dir entry and does not
/// follow symlinks.
async fn read_dir_sorted(path: &Path) -> std::io::Result<Vec<(PathBuf, std::fs::FileType)>> {
    let mut rd = tokio::fs::read_dir(path).await?;
    let mut entries = Vec::new();
    while let Some(entry) = rd.next_entry().await? {
        let ft = entry.file_type().await?;
        entries.push((entry.path(), ft));
    }
    entries.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()));
    Ok(entries)
}

/// `(dir-chan path)` / `(dir-chan path cap)` — a channel of the directory's
/// entry maps in name order, closed after the last one.
fn builtin_dir_chan(args: &[Value]) -> ValueResult<Value> {
    let path = str_arg(args, 0, "string (path)")?;
    let cap = size_arg_or(args, 1, DEFAULT_STREAM_CAP)?;
    let ch = make_chan(cap);
    let ch_val = Value::NativeObject(ch.clone());
    spawn_future(async move {
        match read_dir_sorted(Path::new(&path)).await {
            Ok(entries) => {
                for (p, ft) in entries {
                    if !put(&ch, entry_value(&p, ft, None)).await {
                        break; // consumer closed the channel
                    }
                }
            }
            Err(e) => {
                put(&ch, io_error(format!("cannot list {path}: {e}"))).await;
            }
        }
        chan_ref(ch.get()).close();
        Ok(Value::Nil)
    });
    Ok(ch_val)
}

/// `(walk-chan path)` / `(walk-chan path cap)` — a channel of the root entry
/// and everything beneath it, depth-first with children in name order.
/// Symlinked directories are reported but not descended into.
fn builtin_walk_chan(args: &[Value]) -> ValueResult<Value> {
    let root = str_arg(args, 0, "string (path)")?;
    let cap = size_arg_or(args, 1, DEFAULT_STREAM_CAP)?;
    let ch = make_chan(cap);
    let ch_val = Value::NativeObject(ch.clone());
    spawn_future(async move {
        let root_path = PathBuf::from(&root);
        let root_ft = match tokio::fs::symlink_metadata(&root_path).await {
            Ok(meta) => meta.file_type(),
            Err(e) => {
                put(&ch, io_error(format!("cannot stat {root}: {e}"))).await;
                chan_ref(ch.get()).close();
                return Ok(Value::Nil);
            }
        };
        // Depth-first via an explicit stack; children are pushed in reverse
        // name order so popping yields them in name order.
        let mut stack: Vec<(PathBuf, std::fs::FileType, i64)> = vec![(root_path, root_ft, 0)];
        'walk: while let Some((path, ft, depth)) = stack.pop() {
            if !put(&ch, entry_value(&path, ft, Some(depth))).await {
                break;
            }
            if !ft.is_dir() {
                continue;
            }
            match read_dir_sorted(&path).await {
                Ok(mut entries) => {
                    entries.reverse();
                    for (p, ft) in entries {
                        stack.push((p, ft, depth + 1));
                    }
                }
                Err(e) => {
                    let msg = format!("cannot list {}: {e}", path.display());
                    if !put(&ch, io_error(msg)).await {
                        break 'walk;
                    }
                }
            }
        }
        chan_ref(ch.get()).close();
        Ok(Value::Nil)
    });
    Ok(ch_val)
}

// ── Metadata ──────────────────────────────────────────────────────────────────

/// `(stat path)` — a promise channel delivering
/// `{:path :name :type :size :readonly}` plus `:modified` (epoch millis) when
/// the platform reports it. Symlinks are not followed.
fn builtin_stat(args: &[Value]) -> ValueResult<Value> {
    let path = str_arg(args, 0, "string (path)")?;
    let ch = make_chan(1);
    let ch_val = Value::NativeObject(ch.clone());
    spawn_future(async move {
        let result = match tokio::fs::symlink_metadata(&path).await {
            Ok(meta) => {
                let p = Path::new(&path);
                let mut pairs = vec![
                    (kw("path"), Value::string(path.clone())),
                    (kw("name"), Value::string(entry_name(p))),
                    (kw("type"), type_keyword(meta.file_type())),
                    (kw("size"), Value::Long(meta.len() as i64)),
                    (kw("readonly"), Value::Bool(meta.permissions().readonly())),
                ];
                if let Ok(modified) = meta.modified() {
                    if let Ok(d) = modified.duration_since(UNIX_EPOCH) {
                        pairs.push((kw("modified"), Value::Long(d.as_millis() as i64)));
                    }
                }
                Value::Map(MapValue::from_pairs(pairs))
            }
            Err(e) => io_error(format!("cannot stat {path}: {e}")),
        };
        deliver(&ch, result).await;
        Ok(Value::Nil)
    });
    Ok(ch_val)
}
