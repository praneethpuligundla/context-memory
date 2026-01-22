//! Client mode - bridges stdio to daemon socket
//!
//! The client connects to the daemon via Unix socket and bridges
//! stdin/stdout to the socket, making it transparent to Claude Code.
//! If the daemon isn't running, it auto-starts it.

use crate::daemon::{pid_file_path, socket_path};
use std::process::{Command, Stdio};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// Check if the daemon is running by checking the PID file.
pub fn is_daemon_running() -> bool {
    let pid_path = pid_file_path();
    if !pid_path.exists() {
        return false;
    }

    if let Ok(pid_str) = std::fs::read_to_string(&pid_path) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            // On macOS, use kill -0 to check if process exists
            // On Linux, we could check /proc/{pid} but kill -0 works everywhere
            return Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
        }
    }
    false
}

/// Start the daemon process in the background.
pub fn start_daemon() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;

    // Spawn daemon as a detached background process
    Command::new(&exe)
        .arg("--daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit()) // Keep stderr for logging
        .spawn()?;

    // Wait for socket to be ready (up to 5 seconds)
    let socket = socket_path();
    for _ in 0..50 {
        if socket.exists() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    anyhow::bail!("Daemon failed to start within 5 seconds")
}

/// Run in client mode - bridge stdio to daemon socket.
///
/// This function:
/// 1. Auto-starts the daemon if not running
/// 2. Connects to the daemon via Unix socket
/// 3. Bridges stdin→socket and socket→stdout
pub async fn run_client() -> anyhow::Result<()> {
    // Auto-start daemon if needed
    if !is_daemon_running() {
        tracing::info!("Daemon not running, starting...");
        start_daemon()?;
        tracing::info!("Daemon started");
    }

    // Connect to daemon
    let socket = socket_path();
    let stream = UnixStream::connect(&socket).await?;
    let (mut socket_read, mut socket_write) = stream.into_split();

    // Bridge stdin/stdout to socket
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    // Use tokio::select! to handle both directions concurrently
    // When either direction ends, the client exits
    tokio::select! {
        // stdin → socket
        result = async {
            let mut buf = vec![0u8; 8192];
            loop {
                let n = stdin.read(&mut buf).await?;
                if n == 0 {
                    break; // EOF on stdin
                }
                socket_write.write_all(&buf[..n]).await?;
                socket_write.flush().await?;
            }
            Ok::<_, std::io::Error>(())
        } => {
            if let Err(e) = result {
                tracing::debug!("stdin→socket ended: {}", e);
            }
        }

        // socket → stdout
        result = async {
            let mut buf = vec![0u8; 8192];
            loop {
                let n = socket_read.read(&mut buf).await?;
                if n == 0 {
                    break; // Socket closed
                }
                stdout.write_all(&buf[..n]).await?;
                stdout.flush().await?;
            }
            Ok::<_, std::io::Error>(())
        } => {
            if let Err(e) = result {
                tracing::debug!("socket→stdout ended: {}", e);
            }
        }
    }

    Ok(())
}
