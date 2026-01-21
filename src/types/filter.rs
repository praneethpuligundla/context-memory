//! Filter and query types for searching facts.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::enums::{Category, Certainty, Importance, Scope, SourceType};
use super::fact::Fact;

/// Filters for querying facts.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FactFilter {
    /// Filter to a specific project path. If not set and all_projects is false,
    /// defaults to current project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    /// If true, search across all projects. If false (default), filter to current project.
    #[serde(default)]
    pub all_projects: bool,
    /// Filter to a specific session ID. If set, only returns facts from that session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<Category>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub importance: Option<Importance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_type: Option<SourceType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_confidence: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certainty: Option<Certainty>,
}

/// A query for recalling facts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallQuery {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub filter: FactFilter,
}

fn default_limit() -> usize {
    10
}

/// Summary of facts grouped by topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicSummary {
    pub topic: String,
    pub count: usize,
    pub facts: Vec<Fact>,
}

/// A pair of contradicting facts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contradiction {
    pub fact_a: Fact,
    pub fact_b: Fact,
    pub reason: String,
}

/// A checkpoint for session management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub fact_count: usize,
}

/// Context for a task being worked on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContext {
    pub id: Uuid,
    pub description: String,
    pub started_at: DateTime<Utc>,
    pub facts: Vec<Uuid>,
}
