//! Input validation functions and security constants.

use crate::error::{MemoryError, Result};

// ============================================================================
// Security Constants
// ============================================================================

/// Maximum length for fact content (64KB).
pub const MAX_CONTENT_LENGTH: usize = 65536;

/// Maximum length for a single topic (256 chars).
pub const MAX_TOPIC_LENGTH: usize = 256;

/// Maximum number of topics per fact.
pub const MAX_TOPICS_PER_FACT: usize = 50;

/// Maximum length for source path (1KB).
pub const MAX_SOURCE_LENGTH: usize = 1024;

/// Maximum length for evidence entry (4KB).
pub const MAX_EVIDENCE_LENGTH: usize = 4096;

/// Maximum number of evidence entries per fact.
pub const MAX_EVIDENCE_PER_FACT: usize = 100;

/// Maximum search query length (1KB).
pub const MAX_QUERY_LENGTH: usize = 1024;

/// Maximum results per query.
pub const MAX_RESULTS_LIMIT: usize = 1000;

// ============================================================================
// Validation Functions
// ============================================================================

/// Validate content length.
pub fn validate_content(content: &str) -> Result<()> {
    if content.is_empty() {
        return Err(MemoryError::ValidationError(
            "Content cannot be empty".to_string(),
        ));
    }
    if content.len() > MAX_CONTENT_LENGTH {
        return Err(MemoryError::ValidationError(format!(
            "Content exceeds maximum length of {} bytes",
            MAX_CONTENT_LENGTH
        )));
    }
    Ok(())
}

/// Validate a single topic.
pub fn validate_topic(topic: &str) -> Result<()> {
    if topic.is_empty() {
        return Err(MemoryError::ValidationError(
            "Topic cannot be empty".to_string(),
        ));
    }
    if topic.len() > MAX_TOPIC_LENGTH {
        return Err(MemoryError::ValidationError(format!(
            "Topic exceeds maximum length of {} chars",
            MAX_TOPIC_LENGTH
        )));
    }
    // Only allow alphanumeric, dash, underscore, and dot
    if !topic
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(MemoryError::ValidationError(
            "Topic contains invalid characters (only alphanumeric, -, _, . allowed)".to_string(),
        ));
    }
    Ok(())
}

/// Validate a list of topics.
pub fn validate_topics(topics: &[String]) -> Result<()> {
    if topics.len() > MAX_TOPICS_PER_FACT {
        return Err(MemoryError::ValidationError(format!(
            "Too many topics (max {})",
            MAX_TOPICS_PER_FACT
        )));
    }
    for topic in topics {
        validate_topic(topic)?;
    }
    Ok(())
}

/// Validate source path.
pub fn validate_source(source: &str) -> Result<()> {
    if source.len() > MAX_SOURCE_LENGTH {
        return Err(MemoryError::ValidationError(format!(
            "Source path exceeds maximum length of {} chars",
            MAX_SOURCE_LENGTH
        )));
    }
    // Check for null bytes which could cause issues
    if source.contains('\0') {
        return Err(MemoryError::ValidationError(
            "Source path contains null bytes".to_string(),
        ));
    }
    Ok(())
}

/// Validate evidence entries.
pub fn validate_evidence(evidence: &[String]) -> Result<()> {
    if evidence.len() > MAX_EVIDENCE_PER_FACT {
        return Err(MemoryError::ValidationError(format!(
            "Too many evidence entries (max {})",
            MAX_EVIDENCE_PER_FACT
        )));
    }
    for entry in evidence {
        if entry.len() > MAX_EVIDENCE_LENGTH {
            return Err(MemoryError::ValidationError(format!(
                "Evidence entry exceeds maximum length of {} chars",
                MAX_EVIDENCE_LENGTH
            )));
        }
    }
    Ok(())
}

/// Validate search query.
pub fn validate_query(query: &str) -> Result<()> {
    if query.len() > MAX_QUERY_LENGTH {
        return Err(MemoryError::ValidationError(format!(
            "Query exceeds maximum length of {} chars",
            MAX_QUERY_LENGTH
        )));
    }
    Ok(())
}

/// Validate and sanitize limit parameter.
pub fn validate_limit(limit: usize) -> usize {
    limit.min(MAX_RESULTS_LIMIT)
}
