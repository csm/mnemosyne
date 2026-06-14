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

/// Guardrail policy for the agent's access to the host environment.
///
/// Defaults to denying everything: a runtime spawned with [`IoPolicy::default`]
/// has no file or network capability and runs purely synchronously (no async
/// executor). Capabilities are opt-in and additive. Enabling file IO or
/// networking implies the async substrate, since both deliver their results
/// over `clojure.core.async` channels.
///
/// Shelling out / executing arbitrary programs is intentionally not
/// representable here — that capability does not exist.
#[derive(Debug, Clone, Default)]
pub struct IoPolicy {
    /// Load `clojure.core.async` (channels, `^:async`, `await`).
    pub async_enabled: bool,
    /// Load async file IO (`clojure.rust.io.async`).
    pub file_io: bool,
    /// Load networking (`clojure.rust.net.*`).
    pub network: bool,
}

impl IoPolicy {
    /// Deny every host capability (the default).
    pub fn deny_all() -> Self {
        Self::default()
    }

    /// Enable the async substrate together with file IO and networking.
    pub fn allow_all() -> Self {
        Self {
            async_enabled: true,
            file_io: true,
            network: true,
        }
    }

    /// Whether any capability requires the async (`current_thread` + `LocalSet`)
    /// executor to be running on the runtime thread.
    fn needs_async_executor(&self) -> bool {
        self.async_enabled || self.file_io || self.network
    }
}

/// Install the gated async / IO / networking substrate into a runtime's
/// globals, according to `policy`. Must run on a thread with a Tokio
/// `current_thread` + `LocalSet` executor active.
fn install_substrate(rt: &ClojureRuntime, policy: &IoPolicy) {
    // The async runtime is the prerequisite for the channel-based IO and net
    // namespaces, so it is installed whenever any capability is enabled.
    let globals = rt.globals();
    cljrs_async::init(globals);
    if policy.file_io {
        cljrs_io::init(globals);
    }
    if policy.network {
        cljrs_net::init(globals);
    }
}

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
    /// Spawn a runtime thread with the default (deny-all) [`IoPolicy`], building
    /// the [`ClojureRuntime`] with `build`.
    ///
    /// The runtime is constructed *on* the thread because it is not `Send` and
    /// therefore cannot be created elsewhere and moved in.
    pub fn spawn<F>(build: F) -> Self
    where
        F: FnOnce() -> ClojureRuntime + Send + 'static,
    {
        Self::spawn_with_policy(build, IoPolicy::deny_all())
    }

    /// Spawn a runtime thread, building it with `build` and granting it the host
    /// capabilities allowed by `policy`.
    ///
    /// When `policy` enables any capability, the thread hosts a Tokio
    /// `current_thread` + `LocalSet` executor (as required by `cljrs-async`,
    /// `cljrs-io`, and `cljrs-net`) and the job loop runs asynchronously so the
    /// channel-backed IO/network tasks can make progress. Otherwise the thread
    /// runs a plain synchronous loop with no executor.
    pub fn spawn_with_policy<F>(build: F, policy: IoPolicy) -> Self
    where
        F: FnOnce() -> ClojureRuntime + Send + 'static,
    {
        let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
        thread::Builder::new()
            .name("clojure-runtime".into())
            .spawn(move || {
                if policy.needs_async_executor() {
                    let tokio_rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("failed to build current-thread Tokio runtime");
                    let local = tokio::task::LocalSet::new();
                    local.block_on(&tokio_rt, async move {
                        let mut rt = build();
                        install_substrate(&rt, &policy);
                        while let Some(job) = rx.recv().await {
                            handle_job_async(&mut rt, job).await;
                        }
                    });
                } else {
                    let mut rt = build();
                    // `blocking_recv` parks the thread until a job arrives; it
                    // does not require an active Tokio runtime on this thread.
                    while let Some(job) = rx.blocking_recv() {
                        handle_job(&mut rt, job);
                    }
                }
            })
            .expect("failed to spawn clojure-runtime thread");
        Self { tx }
    }

    /// Spawn a runtime with the full standard library loaded (deny-all policy).
    pub fn spawn_full() -> Self {
        Self::spawn(ClojureRuntime::new)
    }

    /// Spawn a runtime with only the minimal bootstrap environment (deny-all).
    pub fn spawn_minimal() -> Self {
        Self::spawn(ClojureRuntime::minimal)
    }

    /// Spawn a full runtime granting the capabilities in `policy`.
    pub fn spawn_full_with_policy(policy: IoPolicy) -> Self {
        Self::spawn_with_policy(ClojureRuntime::new, policy)
    }

    /// Spawn a minimal runtime granting the capabilities in `policy`.
    pub fn spawn_minimal_with_policy(policy: IoPolicy) -> Self {
        Self::spawn_with_policy(ClojureRuntime::minimal, policy)
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

/// Execute a job on the async-executor loop. `Eval` is driven cooperatively via
/// [`ClojureRuntime::eval_async`] so awaiting IO/network channels yields on the
/// LocalSet instead of blocking it; every other job is synchronous.
async fn handle_job_async(rt: &mut ClojureRuntime, job: Job) {
    match job {
        Job::Eval { source, reply } => {
            let _ = reply.send(rt.eval_async(&source).await);
        }
        other => handle_job(rt, other),
    }
}

/// Execute a single job against the runtime, replying on its one-shot channel.
/// Shared by both the synchronous and async-executor job loops.
fn handle_job(rt: &mut ClojureRuntime, job: Job) {
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
