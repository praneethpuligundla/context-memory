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
