//! Utility functions for hashing, topic extraction, and verification.

use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::{LazyLock, OnceLock};

use crate::error::{MemoryError, Result};
use crate::types::Fact;
use crate::validation::{MAX_TOPICS_PER_FACT, MAX_TOPIC_LENGTH};

/// Cached regex for extracting hashtags from content.
static HASHTAG_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"#(\w+)").expect("Invalid hashtag regex"));

/// Tokenize a search query into individual terms.
/// Returns lowercased terms for case-insensitive matching.
pub fn tokenize_query(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split_whitespace()
        .map(|s| s.to_string())
        .collect()
}

/// Compute SHA-256 hash of file content for staleness detection.
///
/// # Security
/// This function validates paths to prevent path traversal attacks.
/// Only relative paths within the current working directory are allowed.
pub fn compute_source_hash(source: &str) -> Option<String> {
    // Parse "path:line" format
    let path_str = source.split(':').next()?;
    let path = Path::new(path_str);

    // Security: Reject absolute paths and paths with traversal components
    if path.is_absolute() {
        return None;
    }

    // Security: Check for path traversal attempts
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => return None, // Reject ".."
            std::path::Component::Prefix(_) => return None, // Reject Windows prefixes
            _ => {}
        }
    }

    // Security: Canonicalize and verify the path stays within cwd
    let cwd = std::env::current_dir().ok()?;
    let full_path = cwd.join(path);
    let canonical = full_path.canonicalize().ok()?;

    // Verify the canonical path is still under cwd
    if !canonical.starts_with(&cwd) {
        return None;
    }

    if canonical.exists() && canonical.is_file() {
        let content = std::fs::read(&canonical).ok()?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        Some(hex::encode(hasher.finalize()))
    } else {
        None
    }
}

/// Extract topics from content using simple heuristics.
pub fn extract_topics(content: &str) -> Vec<String> {
    let mut topics = Vec::new();

    // Look for hashtags using cached regex
    for cap in HASHTAG_REGEX.captures_iter(content) {
        if let Some(tag) = cap.get(1) {
            let topic = tag.as_str().to_lowercase();
            // Apply validation rules: only alphanumeric, dash, underscore, dot
            if topic.len() <= MAX_TOPIC_LENGTH
                && topic
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
            {
                topics.push(topic);
            }
        }
    }

    // Look for common keywords
    const KEYWORDS: &[&str] = &[
        "auth",
        "api",
        "database",
        "ui",
        "test",
        "config",
        "error",
        "performance",
        "security",
        "deploy",
    ];
    let content_lower = content.to_lowercase();
    for kw in KEYWORDS {
        if content_lower.contains(kw) {
            topics.push((*kw).to_string());
        }
    }

    topics.sort();
    topics.dedup();

    // Cap the number of auto-extracted topics
    topics.truncate(MAX_TOPICS_PER_FACT);
    topics
}

/// Run a command with a timeout to prevent hangs.
fn run_command_with_timeout(cmd: &mut std::process::Command, timeout_ms: u64) -> Option<std::process::Output> {
    use std::time::{Duration, Instant};

    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;

    let start = Instant::now();
    let timeout = Duration::from_millis(timeout_ms);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Process finished
                let stdout = child.stdout.take().and_then(|mut s| {
                    use std::io::Read;
                    let mut buf = Vec::new();
                    s.read_to_end(&mut buf).ok().map(|_| buf)
                }).unwrap_or_default();
                let stderr = child.stderr.take().and_then(|mut s| {
                    use std::io::Read;
                    let mut buf = Vec::new();
                    s.read_to_end(&mut buf).ok().map(|_| buf)
                }).unwrap_or_default();

                return Some(std::process::Output { status, stdout, stderr });
            }
            Ok(None) => {
                // Still running
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return None,
        }
    }
}

/// Get current git commit hash if in a git repository.
/// Times out after 2 seconds to prevent hangs on slow repos.
pub fn get_git_commit() -> Option<String> {
    run_command_with_timeout(
        std::process::Command::new("git").args(["rev-parse", "HEAD"]),
        2000,
    )
    .and_then(|output| {
        if output.status.success() {
            String::from_utf8(output.stdout)
                .ok()
                .map(|s| s.trim().to_string())
        } else {
            None
        }
    })
}

/// Cached project root to avoid repeated subprocess calls.
static PROJECT_ROOT_CACHE: OnceLock<Option<String>> = OnceLock::new();

/// Get the project root directory (cached).
///
/// Tries to find the git repository root, falling back to the current working directory.
/// Returns a canonicalized absolute path (resolves symlinks).
/// The result is cached for the lifetime of the process.
pub fn get_project_root() -> Option<String> {
    PROJECT_ROOT_CACHE
        .get_or_init(|| {
            // Try git root first
            if let Some(git_root) = get_git_root() {
                return canonicalize_path(&git_root);
            }

            // Fall back to current working directory
            std::env::current_dir()
                .ok()
                .and_then(|p| p.canonicalize().ok())
                .map(|p| p.to_string_lossy().to_string())
        })
        .clone()
}

/// Get the git repository root directory.
/// Times out after 2 seconds to prevent hangs on slow repos.
fn get_git_root() -> Option<String> {
    run_command_with_timeout(
        std::process::Command::new("git").args(["rev-parse", "--show-toplevel"]),
        2000,
    )
    .and_then(|output| {
        if output.status.success() {
            String::from_utf8(output.stdout)
                .ok()
                .map(|s| s.trim().to_string())
        } else {
            None
        }
    })
}

/// Canonicalize a path (resolve symlinks, normalize).
///
/// Returns None if the path doesn't exist or can't be canonicalized.
pub fn canonicalize_path(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .canonicalize()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

/// Verify if a fact's source has changed.
pub fn verify_source(fact: &Fact) -> Result<bool> {
    let Some(source) = &fact.source else {
        return Ok(true); // No source = nothing to verify
    };

    let Some(stored_hash) = &fact.source_content_hash else {
        return Ok(true); // No hash stored = can't verify
    };

    let current_hash = compute_source_hash(source);

    match current_hash {
        Some(hash) => Ok(hash == *stored_hash),
        None => Err(MemoryError::VerificationFailed(format!(
            "Could not read source: {}",
            source
        ))),
    }
}
