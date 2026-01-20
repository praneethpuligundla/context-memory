//! Context Memory MCP Server
//!
//! A persistent, context-rot-resistant memory system for Claude Code.
//!
//! Run as MCP server:
//!   context-memory
//!
//! The server uses stdio transport and stores data at ~/.claude/context-memory/memory.db

use tracing_subscriber::EnvFilter;

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

    // Determine database path
    let db_path = dirs::home_dir()
        .map(|h| h.join(".claude").join("context-memory").join("memory.db"))
        .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;

    let db_path_str = db_path.to_string_lossy().to_string();

    // Run the MCP server
    context_memory::server::run_server(&db_path_str).await
}
