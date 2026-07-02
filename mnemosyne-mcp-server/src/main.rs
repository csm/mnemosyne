//! Self-contained MCP server for Mnemosyne.
//!
//! Speaks MCP over stdio (one JSON-RPC message per line), so it plugs
//! directly into any MCP client configuration:
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "mnemosyne": {
//!       "command": "mnemosyne-mcp-server",
//!       "args": ["--data-dir", "/path/to/state"]
//!     }
//!   }
//! }
//! ```
//!
//! All logging goes to stderr; stdout is reserved for the protocol.

use std::path::PathBuf;
use std::process::ExitCode;

use mnemosyne_mcp::{build_server, IoPolicy, McpConfig};
use tracing_subscriber::EnvFilter;

const USAGE: &str = "\
mnemosyne-mcp-server — MCP server exposing Mnemosyne over stdio

USAGE:
    mnemosyne-mcp-server [OPTIONS]

OPTIONS:
    --data-dir <PATH>   Directory for persistent state: the internal git
                        repository of saved functions, search indexes, and
                        the external repo cache.
                        [default: $MNEMOSYNE_DATA_DIR or ./.mnemosyne]
    --allow-file-io     Grant the Clojure eval runtime file IO
    --allow-network     Grant the Clojure eval runtime network access
    --allow-all         Grant every host capability (file IO + network)
    --minimal-runtime   Boot without the full Clojure standard library
                        (faster start, fewer functions available to eval)
    -h, --help          Print this help
    -V, --version       Print version

Logging is controlled with RUST_LOG (default: info), written to stderr.";

fn parse_config(args: &[String]) -> Result<Option<McpConfig>, String> {
    let mut config = McpConfig {
        data_dir: std::env::var_os("MNEMOSYNE_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./.mnemosyne")),
        ..McpConfig::default()
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--data-dir" => {
                i += 1;
                let path = args
                    .get(i)
                    .ok_or_else(|| "--data-dir requires a path".to_owned())?;
                config.data_dir = PathBuf::from(path);
            }
            "--allow-file-io" => {
                config.io_policy.async_enabled = true;
                config.io_policy.file_io = true;
            }
            "--allow-network" => {
                config.io_policy.async_enabled = true;
                config.io_policy.network = true;
            }
            "--allow-all" => config.io_policy = IoPolicy::allow_all(),
            "--minimal-runtime" => config.minimal_runtime = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("mnemosyne-mcp-server {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            other => return Err(format!("unknown argument: {other}\n\n{USAGE}")),
        }
        i += 1;
    }
    Ok(Some(config))
}

#[tokio::main]
async fn main() -> ExitCode {
    // stdout carries the protocol; everything else goes to stderr.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(std::io::stderr)
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = match parse_config(&args) {
        Ok(Some(c)) => c,
        Ok(None) => return ExitCode::SUCCESS, // --help / --version
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };

    tracing::info!(
        data_dir = %config.data_dir.display(),
        file_io = config.io_policy.file_io,
        network = config.io_policy.network,
        "starting mnemosyne-mcp-server"
    );

    let server = match build_server(config).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("initialisation failed: {e:#}");
            return ExitCode::FAILURE;
        }
    };

    match server.run_stdio().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("transport error: {e}");
            ExitCode::FAILURE
        }
    }
}
