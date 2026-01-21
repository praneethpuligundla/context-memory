//! Input and output types for MCP tools.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{Category, Certainty, Fact, Importance, RelationType, Scope, SourceType};

// ============================================================================
// Input Types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct RememberInput {
    pub content: String,
    pub source: Option<String>,
    pub source_type: Option<SourceType>,
    pub topics: Option<Vec<String>>,
    pub category: Option<Category>,
    pub importance: Option<Importance>,
    pub certainty: Option<Certainty>,
    pub confidence: Option<f32>,
    pub scope: Option<Scope>,
    pub evidence: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct RecallInput {
    pub query: String,
    pub limit: Option<usize>,
    pub filter: Option<crate::types::FactFilter>,
}

#[derive(Debug, Deserialize)]
pub struct ForgetInput {
    pub fact_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct ForgetObservationInput {
    pub fact_id: Uuid,
    pub observation: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyInput {
    pub fact_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct GetStaleInput {
    pub threshold_hours: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct LinkInput {
    pub fact_a: Uuid,
    pub fact_b: Uuid,
    pub relation_type: RelationType,
    pub metadata: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UnlinkInput {
    pub fact_a: Uuid,
    pub fact_b: Uuid,
    pub relation_type: RelationType,
}

#[derive(Debug, Deserialize)]
pub struct GetRelatedInput {
    pub fact_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct SummarizeInput {
    pub topic: String,
    pub limit: Option<usize>,
}

// ============================================================================
// Output Types
// ============================================================================

#[derive(Debug, Serialize)]
pub struct RememberOutput {
    pub id: Uuid,
    pub message: String,
    /// Warnings about potential contradictions with existing facts
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ContradictionWarning>,
}

#[derive(Debug, Serialize)]
pub struct ContradictionWarning {
    pub existing_fact_id: Uuid,
    pub existing_fact_content: String,
    pub reason: String,
    /// Whether a contradicts relation was auto-created
    pub relation_created: bool,
}

#[derive(Debug, Serialize)]
pub struct RecallOutput {
    pub facts: Vec<Fact>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct ForgetOutput {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct VerifyOutput {
    pub fact_id: Uuid,
    pub still_valid: bool,
    pub message: String,
    pub new_hash: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GetStaleOutput {
    pub facts: Vec<Fact>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct RefreshAllOutput {
    pub verified: usize,
    pub stale: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct LinkOutput {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct RelatedFact {
    pub fact: Fact,
    pub relation_type: RelationType,
    pub direction: String,
}

#[derive(Debug, Serialize)]
pub struct GetRelatedOutput {
    pub source_id: Uuid,
    pub related: Vec<RelatedFact>,
}

#[derive(Debug, Serialize)]
pub struct ContradictionPair {
    pub fact_a: Fact,
    pub fact_b: Fact,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct FindContradictionsOutput {
    pub contradictions: Vec<ContradictionPair>,
}

#[derive(Debug, Serialize)]
pub struct TopicInfo {
    pub topic: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct ListTopicsOutput {
    pub topics: Vec<TopicInfo>,
}

#[derive(Debug, Serialize)]
pub struct SummarizeOutput {
    pub topic: String,
    pub fact_count: usize,
    pub facts: Vec<Fact>,
}

#[derive(Debug, Serialize)]
pub struct StatsOutput {
    pub total_facts: usize,
    pub total_topics: usize,
    pub stale_facts: usize,
}

// ============================================================================
// Memory Maintenance Types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct DecayInput {
    /// Only decay facts not accessed in this many days (default: 30)
    pub threshold_days: Option<i64>,
    /// Multiply confidence by this factor (default: 0.9, meaning 10% decay)
    pub decay_factor: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct DecayOutput {
    pub facts_affected: usize,
    pub total_confidence_reduction: f32,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct PruneInput {
    /// Prune facts not accessed in this many days (default: 90)
    pub days_unused: Option<i64>,
    /// Only prune facts with confidence below this (default: 0.5)
    pub min_confidence: Option<f32>,
    /// If true, archive instead of delete (default: true)
    pub archive: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct PruneOutput {
    pub facts_pruned: usize,
    pub fact_ids: Vec<Uuid>,
    pub archived: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct ConsolidateInput {
    /// Minimum topic similarity to consider (0.0-1.0, default: 0.5)
    pub similarity_threshold: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct SimilarFactPair {
    pub fact_a: Fact,
    pub fact_b: Fact,
    pub similarity: f32,
}

#[derive(Debug, Serialize)]
pub struct ConsolidateOutput {
    pub similar_pairs: Vec<SimilarFactPair>,
    pub count: usize,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct ArchiveInput {
    pub fact_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct GetArchivedInput {
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct GetArchivedOutput {
    pub facts: Vec<Fact>,
    pub count: usize,
}

// ============================================================================
// Session Types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct GetSessionFactsInput {
    /// Session ID to query. If not provided, uses current session.
    pub session_id: Option<String>,
    /// Maximum facts to return (default: 50)
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct GetSessionFactsOutput {
    pub session_id: String,
    pub facts: Vec<Fact>,
    pub count: usize,
}

// ============================================================================
// History Types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct GetFactHistoryInput {
    pub fact_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct GetFactHistoryOutput {
    pub fact_id: Uuid,
    pub current: Option<Fact>,
    pub history: Vec<crate::types::FactHistoryEntry>,
    pub version_count: usize,
}

// ============================================================================
// Merge Types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct MergeFactsInput {
    /// ID of the first fact to merge
    pub fact_a: Uuid,
    /// ID of the second fact to merge
    pub fact_b: Uuid,
    /// New combined content (if not provided, uses fact_a's content)
    pub merged_content: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MergeFactsOutput {
    /// ID of the newly created merged fact
    pub merged_id: Uuid,
    /// IDs of the archived original facts
    pub archived_ids: Vec<Uuid>,
    pub message: String,
}

// ============================================================================
// Category Summary Types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct GetCategorySummaryInput {
    /// Category to summarize. If not provided, returns all categories.
    pub category: Option<crate::types::Category>,
    /// Maximum facts per category (default: 10)
    pub limit_per_category: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct CategorySummary {
    pub category: crate::types::Category,
    pub fact_count: usize,
    pub top_topics: Vec<String>,
    pub sample_facts: Vec<Fact>,
}

#[derive(Debug, Serialize)]
pub struct GetCategorySummaryOutput {
    pub summaries: Vec<CategorySummary>,
    pub total_categories: usize,
}
