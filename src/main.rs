//! Context Memory MCP Server
//!
//! A persistent, context-rot-resistant memory system for Claude Code.
//!
//! ## Modes
//!
//! - **Default (client)**: Connects to daemon, auto-starting it if needed
//! - **`--daemon`**: Run as background daemon accepting socket connections
//!
//! The daemon architecture allows multiple Claude Code sessions to share
//! the same memory database without lock conflicts.

use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "context-memory")]
#[command(about = "Context-rot-resistant memory for Claude Code")]
#[command(version)]
struct Cli {
    /// Run as daemon (background server accepting socket connections)
    #[arg(long)]
    daemon: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Setup logging to stderr (stdout is for MCP protocol)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("context_memory=info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    if cli.daemon {
        // Determine database path
        let db_path = dirs::home_dir()
            .map(|h| h.join(".claude").join("context-memory").join("memory.db"))
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;

        let db_path_str = db_path.to_string_lossy().to_string();

        // Run as daemon (listens on Unix socket, accepts multiple clients)
        tracing::info!("Starting context-memory daemon");
        tracing::info!("Database: {}", db_path_str);
        context_memory::daemon::run_daemon(&db_path_str).await
    } else {
        // Default: client mode (auto-starts daemon, bridges stdio to socket)
        context_memory::client::run_client().await
    }
}
