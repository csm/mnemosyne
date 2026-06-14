//! A `Send + Sync` handle to a Clojure runtime that lives on its own thread.
//!
//! The clojurust interpreter is single-threaded: its garbage-collected values
//! (`cljrs_gc::GcPtr`) are neither `Send` nor `Sync`, so a [`ClojureRuntime`]
//! cannot be shared across threads or held across an `.await` point in a `Send`
//! future (as required by the async HTTP server). Instead we run the runtime on
//! a dedicated OS thread and talk to it over a channel — the classic actor
//! pattern. Only plain owned data ([`ClojureValue`], `String`) crosses the
//! thread boundary, and that data *is* `Send`.
//!
//! This is also the seam the async IO/networking substrate plugs into: the
//! runtime thread is the single place where `cljrs-async` (`clojure.core.async`)
//! and the `cljrs-io` / `cljrs-net` channel operations are driven.

use std::thread;

use tokio::sync::{mpsc, oneshot};

use crate::{ClojureRuntime, ClojureValue, ExecutionError, Result};

/// A unit of work for the runtime thread. Each variant carries a one-shot
/// channel on which the result is returned to the caller.
enum Job {
    Eval {
        source: String,
        reply: oneshot::Sender<Result<ClojureValue>>,
    },
    LoadVersioned {
        source: String,
        vref: String,
        reply: oneshot::Sender<Result<()>>,
    },
    BindingNames {
        reply: oneshot::Sender<Vec<String>>,
    },
    CurrentNamespace {
        reply: oneshot::Sender<String>,
    },
    SetNamespace {
        ns: String,
        reply: oneshot::Sender<()>,
    },
}

/// A cloneable, `Send + Sync` handle to a Clojure runtime running on a
/// dedicated thread. Dropping every clone closes the channel and lets the
/// runtime thread exit.
#[derive(Clone)]
pub struct RuntimeHandle {
    tx: mpsc::UnboundedSender<Job>,
}

impl RuntimeHandle {
    /// Spawn a runtime thread, building the [`ClojureRuntime`] with `build`.
    ///
    /// The runtime is constructed *on* the thread because it is not `Send` and
    /// therefore cannot be created elsewhere and moved in.
    pub fn spawn<F>(build: F) -> Self
    where
        F: FnOnce() -> ClojureRuntime + Send + 'static,
    {
        let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
        thread::Builder::new()
            .name("clojure-runtime".into())
            .spawn(move || {
                let mut rt = build();
                // `blocking_recv` parks the thread until a job arrives; it does
                // not require an active Tokio runtime on this thread.
                while let Some(job) = rx.blocking_recv() {
                    match job {
                        Job::Eval { source, reply } => {
                            let _ = reply.send(rt.eval(&source));
                        }
                        Job::LoadVersioned {
                            source,
                            vref,
                            reply,
                        } => {
                            let _ = reply.send(rt.load_versioned(&source, &vref));
                        }
                        Job::BindingNames { reply } => {
                            let _ = reply.send(rt.binding_names());
                        }
                        Job::CurrentNamespace { reply } => {
                            let _ = reply.send(rt.current_namespace().to_string());
                        }
                        Job::SetNamespace { ns, reply } => {
                            rt.set_namespace(&ns);
                            let _ = reply.send(());
                        }
                    }
                }
            })
            .expect("failed to spawn clojure-runtime thread");
        Self { tx }
    }

    /// Spawn a runtime with the full standard library loaded.
    pub fn spawn_full() -> Self {
        Self::spawn(ClojureRuntime::new)
    }

    /// Spawn a runtime with only the minimal bootstrap environment.
    pub fn spawn_minimal() -> Self {
        Self::spawn(ClojureRuntime::minimal)
    }

    /// Send `job` and await its reply, mapping a dead runtime thread to
    /// [`ExecutionError::RuntimeGone`].
    async fn request<T>(&self, job: Job, rx: oneshot::Receiver<T>) -> Result<T> {
        self.tx.send(job).map_err(|_| ExecutionError::RuntimeGone)?;
        rx.await.map_err(|_| ExecutionError::RuntimeGone)
    }

    /// Parse and evaluate `source`, returning the value of the last form.
    pub async fn eval(&self, source: impl Into<String>) -> Result<ClojureValue> {
        let (reply, rx) = oneshot::channel();
        self.request(
            Job::Eval {
                source: source.into(),
                reply,
            },
            rx,
        )
        .await?
    }

    /// Load `source` into the runtime and record `vref` as its pinned version.
    pub async fn load_versioned(
        &self,
        source: impl Into<String>,
        vref: impl Into<String>,
    ) -> Result<()> {
        let (reply, rx) = oneshot::channel();
        self.request(
            Job::LoadVersioned {
                source: source.into(),
                vref: vref.into(),
                reply,
            },
            rx,
        )
        .await?
    }

    /// Names of all vars interned in the current namespace.
    pub async fn binding_names(&self) -> Result<Vec<String>> {
        let (reply, rx) = oneshot::channel();
        self.request(Job::BindingNames { reply }, rx).await
    }

    /// The name of the runtime's current namespace.
    pub async fn current_namespace(&self) -> Result<String> {
        let (reply, rx) = oneshot::channel();
        self.request(Job::CurrentNamespace { reply }, rx).await
    }

    /// Switch the current namespace (creating it if necessary).
    pub async fn set_namespace(&self, ns: impl Into<String>) -> Result<()> {
        let (reply, rx) = oneshot::channel();
        self.request(Job::SetNamespace { ns: ns.into(), reply }, rx)
            .await
    }
}
