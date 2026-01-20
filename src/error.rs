//! Error types for the context-memory crate.

use thiserror::Error;
use uuid::Uuid;

/// All possible errors in the context-memory system.
#[derive(Error, Debug)]
pub enum MemoryError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Fact not found: {0}")]
    NotFound(Uuid),

    #[error("Invalid filter: {0}")]
    InvalidFilter(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),

    #[error("Source verification failed: {0}")]
    VerificationFailed(String),

    #[error("Input validation failed: {0}")]
    ValidationError(String),
}

/// Convenience Result type for context-memory operations.
pub type Result<T> = std::result::Result<T, MemoryError>;
