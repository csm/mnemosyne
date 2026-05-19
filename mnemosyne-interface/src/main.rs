use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use mnemosyne_code_search::CodeIndex;
use mnemosyne_execution_engine::ClojureRuntime;
use mnemosyne_inference_engine::InferenceEngine;
use mnemosyne_interface::{run_server, AnthropicBackend, ServerConfig};
use mnemosyne_memory::MemoryStore;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "mnemosyne",
    about = "Mnemosyne agent — HTTP/SSE chat interface",
    long_about = None
)]
struct Args {
    /// Address to listen on.
    #[arg(short, long, default_value = "127.0.0.1:3000", env = "MNEMOSYNE_BIND")]
    bind: std::net::SocketAddr,

    /// Anthropic API key. Falls back to ANTHROPIC_API_KEY env var.
    #[arg(long, env = "ANTHROPIC_API_KEY")]
    api_key: String,

    /// LLM model name sent to the Anthropic API.
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
    /// Defaults to a directory named `mnemosyne-index` inside the system temp dir.
    #[arg(long, env = "MNEMOSYNE_INDEX_DIR")]
    index_dir: Option<PathBuf>,
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

    let llm = Arc::new(AnthropicBackend::new(&args.api_key));
    let runtime = ClojureRuntime::minimal();

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
