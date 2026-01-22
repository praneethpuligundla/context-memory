//! Daemon mode - single long-running server accepting socket connections
//!
//! The daemon listens on a Unix socket and accepts multiple concurrent client connections.
//! Each client gets its own MCP server instance backed by a shared Storage pool.

use crate::server::ContextMemoryServer;
use crate::storage::Storage;
use rmcp::ServiceExt;
use std::path::PathBuf;
use tokio::net::UnixListener;

const SOCKET_PATH: &str = ".claude/context-memory/daemon.sock";
const PID_FILE: &str = ".claude/context-memory/daemon.pid";

/// Get the path to the daemon socket file.
pub fn socket_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(SOCKET_PATH)
}

/// Get the path to the daemon PID file.
pub fn pid_file_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(PID_FILE)
}

/// Run the MCP server in daemon mode.
///
/// Listens on a Unix socket and accepts multiple concurrent client connections.
/// Each client gets its own MCP server instance, but they all share the same
/// Storage pool (connection pooling handles concurrent DB access).
pub async fn run_daemon(db_path: &str) -> anyhow::Result<()> {
    let storage = Storage::new(db_path)?;
    let socket_path = socket_path();

    // Remove stale socket file if it exists
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }

    // Ensure parent directory exists
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Write PID file for client to check if daemon is running
    std::fs::write(pid_file_path(), std::process::id().to_string())?;

    let listener = UnixListener::bind(&socket_path)?;
    tracing::info!("Daemon listening on {:?}", socket_path);

    // Accept connections in a loop
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let storage_clone = storage.clone();

                // Spawn a new task for each client
                tokio::spawn(async move {
                    tracing::debug!("New client connected");

                    let server = ContextMemoryServer::new(storage_clone);

                    // Split the stream for bidirectional async transport
                    // rmcp's IntoTransport automatically handles (AsyncRead, AsyncWrite) tuples
                    let (read_half, write_half) = stream.into_split();

                    // Serve the client - tuple (R, W) implements IntoTransport
                    match server.serve((read_half, write_half)).await {
                        Ok(service) => {
                            if let Err(e) = service.waiting().await {
                                tracing::debug!("Client session ended: {}", e);
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to serve client: {}", e);
                        }
                    }

                    tracing::debug!("Client disconnected");
                });
            }
            Err(e) => {
                tracing::error!("Failed to accept connection: {}", e);
            }
        }
    }
}
