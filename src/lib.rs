//! Context Memory - Context-rot-resistant memory MCP server for Claude Code.
//!
//! Provides persistent, source-aware memory with staleness detection and contradiction awareness.
//!
//! # Module Structure
//!
//! - [`error`] - Error types and Result alias
//! - [`types`] - Core data structures (Fact, Relation, enums)
//! - [`validation`] - Input validation functions and security constants
//! - [`utils`] - Utility functions for hashing, topic extraction
//! - [`storage`] - SQLite storage layer with FTS5 search
//! - [`server`] - MCP server implementation
//! - [`tools`] - MCP tool handlers

pub mod error;
pub mod server;
pub mod storage;
pub mod tools;
pub mod types;
pub mod utils;
pub mod validation;

// Re-export commonly used items at crate root for convenience
pub use error::{MemoryError, Result};
pub use types::*;
pub use utils::{canonicalize_path, compute_source_hash, extract_topics, get_git_commit, get_project_root, tokenize_query, verify_source};
pub use validation::*;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fact_builder() {
        let fact = Fact::new("Test fact")
            .with_source("src/main.rs:42")
            .with_topics(vec!["test".into(), "example".into()])
            .with_category(Category::Pattern)
            .with_importance(Importance::High)
            .with_confidence(0.9);

        assert_eq!(fact.content, "Test fact");
        assert_eq!(fact.source, Some("src/main.rs:42".to_string()));
        assert_eq!(fact.source_type, SourceType::Code);
        assert_eq!(fact.topics, vec!["test", "example"]);
        assert_eq!(fact.category, Category::Pattern);
        assert_eq!(fact.importance, Importance::High);
        assert!((fact.confidence - 0.9).abs() < 0.001);
    }

    #[test]
    fn test_relation_builder() {
        let id_a = uuid::Uuid::new_v4();
        let id_b = uuid::Uuid::new_v4();

        let relation = Relation::new(id_a, id_b, RelationType::DependsOn)
            .with_metadata("B is required by A");

        assert_eq!(relation.from_id, id_a);
        assert_eq!(relation.to_id, id_b);
        assert_eq!(relation.relation_type, RelationType::DependsOn);
        assert_eq!(relation.metadata, Some("B is required by A".to_string()));
    }

    #[test]
    fn test_extract_topics() {
        let content = "This is about #authentication and API security";
        let topics = extract_topics(content);

        assert!(topics.contains(&"authentication".to_string()));
        assert!(topics.contains(&"api".to_string()));
        assert!(topics.contains(&"security".to_string()));
    }
}
