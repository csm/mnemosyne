use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use mnemosyne_code_search::CodeIndex;
use mnemosyne_execution_engine::{IoPolicy, RuntimeHandle};
use mnemosyne_inference_engine::{InferenceEngine, LlmBackend};
use mnemosyne_interface::{run_server, AnthropicBackend, OpenAiCompatBackend, ServerConfig};
use mnemosyne_memory::MemoryStore;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "mnemosyne",
    about = "Mnemosyne agent — HTTP/SSE chat interface",
    long_about = "\
Starts an HTTP server with a chat UI at GET / and a streaming SSE endpoint at \
POST /api/chat.\n\n\
Backend selection (pick one):\n  \
  --api-key / ANTHROPIC_API_KEY  →  Anthropic Messages API\n  \
  --base-url                     →  any OpenAI-compatible server\n                               \
     (Ollama, llama.cpp, LM Studio, …)"
)]
struct Args {
    /// Address to listen on.
    #[arg(short, long, default_value = "127.0.0.1:3000", env = "MNEMOSYNE_BIND")]
    bind: std::net::SocketAddr,

    /// Anthropic API key. Required when --base-url is not set.
    #[arg(long, env = "ANTHROPIC_API_KEY")]
    api_key: Option<String>,

    /// OpenAI-compatible base URL, e.g. http://localhost:11434/v1 (Ollama)
    /// or http://localhost:8080/v1 (llama.cpp). When set, --api-key is optional.
    #[arg(long, env = "MNEMOSYNE_BASE_URL")]
    base_url: Option<String>,

    /// LLM model name passed through to the backend.
    #[arg(
        short,
        long,
        default_value = "claude-opus-4-7",
        env = "MNEMOSYNE_MODEL"
    )]
    model: String,

    /// Directory for episodic memory logs.
    /// If omitted, memory logging is disabled.
    #[arg(long, env = "MNEMOSYNE_MEMORY_DIR")]
    memory_dir: Option<PathBuf>,

    /// Directory for the Tantivy code index.
    /// Defaults to a `mnemosyne-index` folder inside the system temp dir.
    #[arg(long, env = "MNEMOSYNE_INDEX_DIR")]
    index_dir: Option<PathBuf>,

    /// Allow the agent to perform async file IO (`clojure.rust.io.async`).
    /// Implies the async substrate. Off by default.
    #[arg(long, env = "MNEMOSYNE_ALLOW_FILE_IO")]
    allow_file_io: bool,

    /// Allow the agent to perform networking (`clojure.rust.net.*`).
    /// Implies the async substrate. Off by default.
    #[arg(long, env = "MNEMOSYNE_ALLOW_NETWORK")]
    allow_network: bool,

    /// Load `clojure.core.async` (channels, `^:async`, `await`) without granting
    /// file or network access. Off by default.
    #[arg(long, env = "MNEMOSYNE_ALLOW_ASYNC")]
    allow_async: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("mnemosyne=info".parse()?)
                .add_directive("tower_http=info".parse()?),
        )
        .init();

    let args = Args::parse();

    let llm: Arc<dyn LlmBackend> = match (args.base_url, args.api_key) {
        (Some(url), key) => {
            let key = key.unwrap_or_else(|| "local".to_owned());
            tracing::info!("using OpenAI-compatible backend at {url}");
            Arc::new(OpenAiCompatBackend::new(url, key))
        }
        (None, Some(key)) => {
            tracing::info!("using Anthropic backend");
            Arc::new(AnthropicBackend::new(key))
        }
        (None, None) => {
            anyhow::bail!(
                "no LLM backend configured — provide --api-key (Anthropic) \
                 or --base-url (OpenAI-compatible local server)"
            );
        }
    };

    let policy = IoPolicy {
        async_enabled: args.allow_async,
        file_io: args.allow_file_io,
        network: args.allow_network,
    };
    if args.allow_async || args.allow_file_io || args.allow_network {
        tracing::info!(
            file_io = args.allow_file_io,
            network = args.allow_network,
            "environment access enabled"
        );
    } else {
        tracing::info!(
            "environment access disabled (deny-all); pass --allow-file-io / \
             --allow-network to grant capabilities"
        );
    }
    let runtime = RuntimeHandle::spawn_minimal_with_policy(policy);

    let index_dir = args
        .index_dir
        .unwrap_or_else(|| std::env::temp_dir().join("mnemosyne-index"));
    let index = CodeIndex::open_or_create(&index_dir)?;

    let mut engine = InferenceEngine::new(llm, runtime, index);
    engine.default_model = args.model;

    let engine = if let Some(dir) = args.memory_dir {
        let store = MemoryStore::create(&dir)?;
        engine.with_memory(store)
    } else {
        engine
    };

    let config = ServerConfig { bind: args.bind };
    run_server(Arc::new(engine), config).await?;

    Ok(())
}
