//! Fact data structure - the core unit of memory.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::enums::{Category, Certainty, Importance, Scope, SourceType};
use crate::utils::compute_source_hash;

/// A fact stored in memory with full provenance tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    pub id: Uuid,
    pub content: String,

    // Source provenance
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub source_type: SourceType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_content_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,

    // Confidence & lifecycle
    pub confidence: f32,
    pub certainty: Certainty,
    pub created_at: DateTime<Utc>,
    pub last_verified: DateTime<Utc>,
    pub stale: bool,

    // Categorization
    pub topics: Vec<String>,
    pub category: Category,
    pub importance: Importance,
    pub scope: Scope,

    // Provenance chain
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derived_from: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<Uuid>,
    pub evidence: Vec<String>,

    // Usage tracking
    pub access_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_accessed: Option<DateTime<Utc>>,
}

impl Fact {
    /// Create a new fact with default metadata.
    pub fn new(content: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            content: content.into(),
            source: None,
            source_type: SourceType::default(),
            source_content_hash: None,
            git_commit: None,
            confidence: 0.8,
            certainty: Certainty::default(),
            created_at: now,
            last_verified: now,
            stale: false,
            topics: Vec::new(),
            category: Category::default(),
            importance: Importance::default(),
            scope: Scope::default(),
            derived_from: None,
            supersedes: None,
            evidence: Vec::new(),
            access_count: 0,
            last_accessed: None,
        }
    }

    /// Set the source location for this fact.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        let source_str = source.into();
        // Auto-detect source type from path
        if source_str.contains(':') && !source_str.starts_with("http") {
            self.source_type = SourceType::Code;
            // Try to compute hash of source file
            if let Some(hash) = compute_source_hash(&source_str) {
                self.source_content_hash = Some(hash);
            }
        }
        self.source = Some(source_str);
        self
    }

    pub fn with_source_type(mut self, source_type: SourceType) -> Self {
        self.source_type = source_type;
        self
    }

    pub fn with_topics(mut self, topics: Vec<String>) -> Self {
        self.topics = topics;
        self
    }

    pub fn with_category(mut self, category: Category) -> Self {
        self.category = category;
        self
    }

    pub fn with_importance(mut self, importance: Importance) -> Self {
        self.importance = importance;
        self
    }

    pub fn with_certainty(mut self, certainty: Certainty) -> Self {
        self.certainty = certainty;
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    pub fn with_scope(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    pub fn with_evidence(mut self, evidence: Vec<String>) -> Self {
        self.evidence = evidence;
        self
    }

    pub fn derived_from(mut self, parent_id: Uuid) -> Self {
        self.derived_from = Some(parent_id);
        self
    }

    pub fn supersedes(mut self, old_id: Uuid) -> Self {
        self.supersedes = Some(old_id);
        self
    }
}
