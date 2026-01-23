//! MCP Server implementation using rmcp (official Rust MCP SDK)

use crate::storage::Storage;
use crate::tools::{
    ArchiveInput, ConsolidateInput, DecayInput, ForgetInput, ForgetObservationInput,
    GetArchivedInput, GetCategorySummaryInput, GetFactHistoryInput, GetRelatedInput,
    GetSessionFactsInput, GetStaleInput, LinkInput, MergeFactsInput, PruneInput, RecallInput,
    RememberInput, SummarizeInput, ToolHandler, UnlinkInput, VerifyInput,
};
use crate::{Category, Certainty, FactFilter, Importance, RelationType, Scope, SourceType};
use rmcp::{
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router,
    ErrorData as McpError, ServerHandler, ServiceExt,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

// ============================================================================
// Parameter structs for tools
// ============================================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RememberParams {
    /// The fact to remember
    pub content: String,
    /// Source location (e.g., 'src/auth.rs:42')
    pub source: Option<String>,
    /// Type of source: code, conversation, manual, inferred
    pub source_type: Option<String>,
    /// Topics/tags for categorization
    pub topics: Option<Vec<String>>,
    /// Category: architecture, decision, pattern, convention, bug, todo, dependency, preference, context
    pub category: Option<String>,
    /// Importance level: critical, high, normal, low
    pub importance: Option<String>,
    /// Certainty: definite, likely, uncertain, speculative
    pub certainty: Option<String>,
    /// Confidence score (0.0-1.0)
    pub confidence: Option<f32>,
    /// Scope: global, project, branch, task
    pub scope: Option<String>,
    /// Supporting evidence
    pub evidence: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecallParams {
    /// Search query (keywords)
    pub query: String,
    /// Maximum results to return
    pub limit: Option<usize>,
    /// Search across all projects (default: false, searches current project only)
    pub all_projects: Option<bool>,
    /// Filter by session ID (only return facts from this session)
    pub session_id: Option<String>,
    /// Filter by topics
    pub topics: Option<Vec<String>>,
    /// Filter by category
    pub category: Option<String>,
    /// Filter by importance
    pub importance: Option<String>,
    /// Filter by scope
    pub scope: Option<String>,
    /// Filter by staleness
    pub stale: Option<bool>,
    /// Minimum confidence
    pub min_confidence: Option<f32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForgetParams {
    /// ID of the fact to remove
    pub fact_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForgetObservationParams {
    /// ID of the fact
    pub fact_id: String,
    /// The observation to remove
    pub observation: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VerifyParams {
    /// ID of the fact to verify
    pub fact_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetStaleParams {
    /// Include facts not verified in this many hours
    pub threshold_hours: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LinkParams {
    /// ID of first fact
    pub fact_a: String,
    /// ID of second fact
    pub fact_b: String,
    /// Type: depends_on, contradicts, elaborates, related_to, part_of, supersedes
    pub relation_type: String,
    /// Additional context about the relationship
    pub metadata: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UnlinkParams {
    /// ID of first fact
    pub fact_a: String,
    /// ID of second fact
    pub fact_b: String,
    /// Type of relationship to remove
    pub relation_type: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetRelatedParams {
    /// ID of the fact to find relations for
    pub fact_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SummarizeParams {
    /// The topic to summarize
    pub topic: String,
    /// Maximum facts to include
    pub limit: Option<usize>,
}

// Memory Maintenance Tools

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DecayParams {
    /// Only decay facts not accessed in this many days (default: 30)
    pub threshold_days: Option<i64>,
    /// Multiply confidence by this factor (default: 0.9, meaning 10% decay)
    pub decay_factor: Option<f32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PruneParams {
    /// Prune facts not accessed in this many days (default: 90)
    pub days_unused: Option<i64>,
    /// Only prune facts with confidence below this (default: 0.5)
    pub min_confidence: Option<f32>,
    /// If true, archive instead of delete (default: true)
    pub archive: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConsolidateParams {
    /// Minimum topic similarity to consider (0.0-1.0, default: 0.5)
    pub similarity_threshold: Option<f32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ArchiveParams {
    /// ID of the fact to archive
    pub fact_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetArchivedParams {
    /// Maximum facts to return (default: 50)
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetSessionFactsParams {
    /// Session ID to query (default: current session)
    pub session_id: Option<String>,
    /// Maximum facts to return (default: 50)
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetFactHistoryParams {
    /// ID of the fact to get history for
    pub fact_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MergeFactsParams {
    /// ID of the first fact to merge
    pub fact_a: String,
    /// ID of the second fact to merge
    pub fact_b: String,
    /// New combined content (if not provided, uses fact_a's content with a note)
    pub merged_content: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetCategorySummaryParams {
    /// Category to summarize. If not provided, returns all categories.
    /// Options: architecture, decision, pattern, convention, bug, todo, dependency, preference, context
    pub category: Option<String>,
    /// Maximum facts per category (default: 10)
    pub limit_per_category: Option<usize>,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Helper to convert a result to a tool result with JSON serialization
fn to_tool_result<T: serde::Serialize>(result: crate::Result<T>) -> Result<CallToolResult, McpError> {
    match result {
        Ok(output) => {
            let json = serde_json::to_string_pretty(&output)
                .map_err(|e| McpError::internal_error(format!("JSON serialization error: {}", e), None))?;
            Ok(CallToolResult::success(vec![Content::text(json)]))
        }
        Err(e) => Err(McpError::internal_error(e.to_string(), None)),
    }
}

// ============================================================================
// MCP Server
// ============================================================================

/// MCP Server for context memory
#[derive(Clone)]
pub struct ContextMemoryServer {
    handler: Arc<Mutex<ToolHandler>>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl ContextMemoryServer {
    pub fn new(storage: Storage) -> Self {
        Self {
            handler: Arc::new(Mutex::new(ToolHandler::new(storage))),
            tool_router: Self::tool_router(),
        }
    }

    // ========================================================================
    // Core Memory Tools
    // ========================================================================

    /// Store a fact in memory with optional source provenance, topics, and metadata
    #[tool(name = "remember")]
    async fn remember(
        &self,
        params: Parameters<RememberParams>,
    ) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let input = RememberInput {
            content: p.content,
            source: p.source,
            source_type: p.source_type.and_then(|s| parse_source_type(&s)),
            topics: p.topics,
            category: p.category.and_then(|s| parse_category(&s)),
            importance: p.importance.and_then(|s| parse_importance(&s)),
            certainty: p.certainty.and_then(|s| parse_certainty(&s)),
            confidence: p.confidence,
            scope: p.scope.and_then(|s| parse_scope(&s)),
            evidence: p.evidence,
        };

        to_tool_result(handler.remember(input))
    }

    /// Search for facts by keyword or topic with optional filters
    #[tool(name = "recall")]
    async fn recall(&self, params: Parameters<RecallParams>) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let filter = FactFilter {
            project_path: None, // Auto-detected from cwd
            all_projects: p.all_projects.unwrap_or(false),
            session_id: p.session_id,
            topics: p.topics,
            category: p.category.and_then(|s| parse_category(&s)),
            importance: p.importance.and_then(|s| parse_importance(&s)),
            scope: p.scope.and_then(|s| parse_scope(&s)),
            source_type: None,
            stale: p.stale,
            min_confidence: p.min_confidence,
            certainty: None,
        };

        let input = RecallInput {
            query: p.query,
            limit: p.limit,
            filter: Some(filter),
        };

        to_tool_result(handler.recall(input))
    }

    /// Remove a fact from memory
    #[tool(name = "forget")]
    async fn forget(&self, params: Parameters<ForgetParams>) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let uuid = uuid::Uuid::parse_str(&p.fact_id)
            .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

        let input = ForgetInput { fact_id: uuid };

        to_tool_result(handler.forget(input))
    }

    /// Remove a specific observation/evidence from a fact
    #[tool(name = "forget_observation")]
    async fn forget_observation(
        &self,
        params: Parameters<ForgetObservationParams>,
    ) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let uuid = uuid::Uuid::parse_str(&p.fact_id)
            .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

        let input = ForgetObservationInput {
            fact_id: uuid,
            observation: p.observation,
        };

        to_tool_result(handler.forget_observation(input))
    }

    // ========================================================================
    // Verification Tools
    // ========================================================================

    /// Re-check if a fact's source file has changed, update staleness
    #[tool(name = "verify")]
    async fn verify(&self, params: Parameters<VerifyParams>) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let uuid = uuid::Uuid::parse_str(&p.fact_id)
            .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

        let input = VerifyInput { fact_id: uuid };

        to_tool_result(handler.verify(input))
    }

    /// List facts that need verification
    #[tool(name = "get_stale")]
    async fn get_stale(
        &self,
        params: Parameters<GetStaleParams>,
    ) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let input = GetStaleInput {
            threshold_hours: p.threshold_hours,
        };

        to_tool_result(handler.get_stale(input))
    }

    /// Batch verify all facts with file sources
    #[tool(name = "refresh_all")]
    async fn refresh_all(&self) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;

        to_tool_result(handler.refresh_all())
    }

    // ========================================================================
    // Relationship Tools
    // ========================================================================

    /// Connect two facts with a relationship
    #[tool(name = "link")]
    async fn link(&self, params: Parameters<LinkParams>) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let uuid_a = uuid::Uuid::parse_str(&p.fact_a)
            .map_err(|e| McpError::invalid_params(format!("Invalid UUID for fact_a: {}", e), None))?;
        let uuid_b = uuid::Uuid::parse_str(&p.fact_b)
            .map_err(|e| McpError::invalid_params(format!("Invalid UUID for fact_b: {}", e), None))?;

        let rel_type = parse_relation_type(&p.relation_type).ok_or_else(|| {
            McpError::invalid_params(format!("Invalid relation_type: {}", p.relation_type), None)
        })?;

        let input = LinkInput {
            fact_a: uuid_a,
            fact_b: uuid_b,
            relation_type: rel_type,
            metadata: p.metadata,
        };

        to_tool_result(handler.link(input))
    }

    /// Remove a relationship between facts
    #[tool(name = "unlink")]
    async fn unlink(&self, params: Parameters<UnlinkParams>) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let uuid_a = uuid::Uuid::parse_str(&p.fact_a)
            .map_err(|e| McpError::invalid_params(format!("Invalid UUID for fact_a: {}", e), None))?;
        let uuid_b = uuid::Uuid::parse_str(&p.fact_b)
            .map_err(|e| McpError::invalid_params(format!("Invalid UUID for fact_b: {}", e), None))?;

        let rel_type = parse_relation_type(&p.relation_type).ok_or_else(|| {
            McpError::invalid_params(format!("Invalid relation_type: {}", p.relation_type), None)
        })?;

        let input = UnlinkInput {
            fact_a: uuid_a,
            fact_b: uuid_b,
            relation_type: rel_type,
        };

        to_tool_result(handler.unlink(input))
    }

    /// Find facts connected to a given fact
    #[tool(name = "get_related")]
    async fn get_related(
        &self,
        params: Parameters<GetRelatedParams>,
    ) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let uuid = uuid::Uuid::parse_str(&p.fact_id)
            .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

        let input = GetRelatedInput { fact_id: uuid };

        to_tool_result(handler.get_related(input))
    }

    /// Detect facts that have been marked as contradicting each other
    #[tool(name = "find_contradictions")]
    async fn find_contradictions(&self) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;

        to_tool_result(handler.find_contradictions())
    }

    // ========================================================================
    // Exploration Tools
    // ========================================================================

    /// List all topics with their fact counts
    #[tool(name = "list_topics")]
    async fn list_topics(&self) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;

        to_tool_result(handler.list_topics())
    }

    /// Get an overview of facts on a specific topic
    #[tool(name = "summarize")]
    async fn summarize(
        &self,
        params: Parameters<SummarizeParams>,
    ) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let input = SummarizeInput {
            topic: p.topic,
            limit: p.limit,
        };

        to_tool_result(handler.summarize(input))
    }

    /// Get memory statistics
    #[tool(name = "stats")]
    async fn stats(&self) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;

        to_tool_result(handler.stats())
    }

    // ========================================================================
    // Memory Maintenance Tools
    // ========================================================================

    /// Apply time-based confidence decay to old, unused facts
    #[tool(name = "decay")]
    async fn decay(&self, params: Parameters<DecayParams>) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let input = DecayInput {
            threshold_days: p.threshold_days,
            decay_factor: p.decay_factor,
        };

        to_tool_result(handler.decay(input))
    }

    /// Prune (archive or delete) old, unused, low-confidence facts
    #[tool(name = "prune")]
    async fn prune(&self, params: Parameters<PruneParams>) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let input = PruneInput {
            days_unused: p.days_unused,
            min_confidence: p.min_confidence,
            archive: p.archive,
        };

        to_tool_result(handler.prune(input))
    }

    /// Find similar facts based on topic overlap for potential consolidation
    #[tool(name = "consolidate")]
    async fn consolidate(
        &self,
        params: Parameters<ConsolidateParams>,
    ) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let input = ConsolidateInput {
            similarity_threshold: p.similarity_threshold,
        };

        to_tool_result(handler.consolidate(input))
    }

    /// Archive a fact (soft-delete, can be restored)
    #[tool(name = "archive")]
    async fn archive(&self, params: Parameters<ArchiveParams>) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let uuid = uuid::Uuid::parse_str(&p.fact_id)
            .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

        let input = ArchiveInput { fact_id: uuid };

        to_tool_result(handler.archive(input))
    }

    /// Restore an archived fact
    #[tool(name = "unarchive")]
    async fn unarchive(
        &self,
        params: Parameters<ArchiveParams>,
    ) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let uuid = uuid::Uuid::parse_str(&p.fact_id)
            .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

        let input = ArchiveInput { fact_id: uuid };

        to_tool_result(handler.unarchive(input))
    }

    /// List archived facts
    #[tool(name = "get_archived")]
    async fn get_archived(
        &self,
        params: Parameters<GetArchivedParams>,
    ) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let input = GetArchivedInput { limit: p.limit };

        to_tool_result(handler.get_archived(input))
    }

    // ========================================================================
    // Session Tools
    // ========================================================================

    /// Get facts from the current or a specific session
    #[tool(name = "get_session_facts")]
    async fn get_session_facts(
        &self,
        params: Parameters<GetSessionFactsParams>,
    ) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let input = GetSessionFactsInput {
            session_id: p.session_id,
            limit: p.limit,
        };

        to_tool_result(handler.get_session_facts(input))
    }

    // ========================================================================
    // History Tools
    // ========================================================================

    /// Get the history of changes for a fact
    #[tool(name = "get_fact_history")]
    async fn get_fact_history(
        &self,
        params: Parameters<GetFactHistoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let uuid = uuid::Uuid::parse_str(&p.fact_id)
            .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))?;

        let input = GetFactHistoryInput { fact_id: uuid };

        to_tool_result(handler.get_fact_history(input))
    }

    // ========================================================================
    // Merge Tools
    // ========================================================================

    /// Merge two similar facts into one, archiving the originals
    #[tool(name = "merge_facts")]
    async fn merge_facts(
        &self,
        params: Parameters<MergeFactsParams>,
    ) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let uuid_a = uuid::Uuid::parse_str(&p.fact_a)
            .map_err(|e| McpError::invalid_params(format!("Invalid UUID for fact_a: {}", e), None))?;
        let uuid_b = uuid::Uuid::parse_str(&p.fact_b)
            .map_err(|e| McpError::invalid_params(format!("Invalid UUID for fact_b: {}", e), None))?;

        let input = MergeFactsInput {
            fact_a: uuid_a,
            fact_b: uuid_b,
            merged_content: p.merged_content,
        };

        to_tool_result(handler.merge_facts(input))
    }

    // ========================================================================
    // Category Summary Tools
    // ========================================================================

    /// Get summaries of facts grouped by category
    #[tool(name = "get_category_summary")]
    async fn get_category_summary(
        &self,
        params: Parameters<GetCategorySummaryParams>,
    ) -> Result<CallToolResult, McpError> {
        let handler = self.handler.lock().await;
        let p = params.0;

        let input = GetCategorySummaryInput {
            category: p.category.and_then(|s| parse_category(&s)),
            limit_per_category: p.limit_per_category,
        };

        to_tool_result(handler.get_category_summary(input))
    }
}

#[tool_handler]
impl ServerHandler for ContextMemoryServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "Context-rot-resistant memory for Claude Code. \
                 PROACTIVE MEMORY: Automatically remember important discoveries, \
                 architectural decisions, user preferences, and notable patterns \
                 without being asked. Use recall at session start to load relevant context. \
                 Tools: remember/recall/forget for facts, verify/get_stale for staleness, \
                 link/get_related for relationships, decay/prune/consolidate for maintenance, \
                 archive/unarchive/get_archived for soft-delete, get_session_facts for session review."
                    .into(),
            ),
        }
    }
}

/// Run the MCP server
pub async fn run_server(db_path: &str) -> anyhow::Result<()> {
    let storage = Storage::new(db_path)?;

    // Run auto-maintenance in background (non-blocking startup)
    let maintenance_storage = storage.clone();
    tokio::spawn(async move {
        // Small delay to let server start first
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if let Err(e) = run_startup_maintenance(&maintenance_storage) {
            tracing::warn!("Background maintenance failed: {}", e);
        }
    });

    let server = ContextMemoryServer::new(storage);

    tracing::info!("Starting context-memory MCP server");
    tracing::info!("Database: {}", db_path);

    let transport = rmcp::transport::io::stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;

    Ok(())
}

/// Run maintenance tasks on server startup.
///
/// Configurable via environment variables:
/// - CONTEXT_MEMORY_DECAY_DAYS: Days of inactivity before decay (default: 7)
/// - CONTEXT_MEMORY_DECAY_FACTOR: Decay multiplier (default: 0.95)
/// - CONTEXT_MEMORY_SKIP_MAINTENANCE: Set to "1" to skip maintenance
fn run_startup_maintenance(storage: &Storage) -> anyhow::Result<()> {
    // Check if maintenance should be skipped
    if std::env::var("CONTEXT_MEMORY_SKIP_MAINTENANCE").unwrap_or_default() == "1" {
        tracing::info!("Skipping startup maintenance (CONTEXT_MEMORY_SKIP_MAINTENANCE=1)");
        return Ok(());
    }

    // Get configuration from environment
    let decay_days: i64 = std::env::var("CONTEXT_MEMORY_DECAY_DAYS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(7);

    let decay_factor: f32 = std::env::var("CONTEXT_MEMORY_DECAY_FACTOR")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.95);

    tracing::info!(
        "Running startup maintenance (decay_days={}, decay_factor={})",
        decay_days,
        decay_factor
    );

    // Apply light decay
    match storage.apply_decay(decay_days, decay_factor) {
        Ok((count, reduction)) => {
            if count > 0 {
                tracing::info!(
                    "Applied decay to {} facts (total confidence reduction: {:.3})",
                    count,
                    reduction
                );
            }
        }
        Err(e) => {
            tracing::warn!("Decay maintenance failed: {}", e);
        }
    }

    Ok(())
}

// ============================================================================
// Parsing helpers
// ============================================================================

fn parse_source_type(s: &str) -> Option<SourceType> {
    match s.to_lowercase().as_str() {
        "code" => Some(SourceType::Code),
        "conversation" => Some(SourceType::Conversation),
        "manual" => Some(SourceType::Manual),
        "inferred" => Some(SourceType::Inferred),
        _ => None,
    }
}

fn parse_category(s: &str) -> Option<Category> {
    match s.to_lowercase().as_str() {
        "architecture" => Some(Category::Architecture),
        "decision" => Some(Category::Decision),
        "pattern" => Some(Category::Pattern),
        "convention" => Some(Category::Convention),
        "bug" => Some(Category::Bug),
        "todo" => Some(Category::Todo),
        "dependency" => Some(Category::Dependency),
        "preference" => Some(Category::Preference),
        "context" => Some(Category::Context),
        _ => None,
    }
}

fn parse_importance(s: &str) -> Option<Importance> {
    match s.to_lowercase().as_str() {
        "critical" => Some(Importance::Critical),
        "high" => Some(Importance::High),
        "normal" => Some(Importance::Normal),
        "low" => Some(Importance::Low),
        _ => None,
    }
}

fn parse_certainty(s: &str) -> Option<Certainty> {
    match s.to_lowercase().as_str() {
        "definite" => Some(Certainty::Definite),
        "likely" => Some(Certainty::Likely),
        "uncertain" => Some(Certainty::Uncertain),
        "speculative" => Some(Certainty::Speculative),
        _ => None,
    }
}

fn parse_scope(s: &str) -> Option<Scope> {
    match s.to_lowercase().as_str() {
        "global" => Some(Scope::Global),
        "project" => Some(Scope::Project),
        "branch" => Some(Scope::Branch),
        "task" => Some(Scope::Task),
        _ => None,
    }
}

fn parse_relation_type(s: &str) -> Option<RelationType> {
    match s.to_lowercase().replace("_", "").as_str() {
        "dependson" => Some(RelationType::DependsOn),
        "contradicts" => Some(RelationType::Contradicts),
        "elaborates" => Some(RelationType::Elaborates),
        "relatedto" => Some(RelationType::RelatedTo),
        "partof" => Some(RelationType::PartOf),
        "supersedes" => Some(RelationType::Supersedes),
        _ => None,
    }
}
