//! MCP Tool implementations.

mod types;

pub use types::*;

use crate::error::{MemoryError, Result};
use crate::storage::Storage;
use crate::types::{Fact, FactFilter, Relation, RelationType};
use crate::utils::{compute_source_hash, extract_topics, get_git_commit, get_project_root, verify_source};
use crate::validation::{
    validate_content, validate_evidence, validate_limit, validate_query, validate_source,
    validate_topics, MAX_RESULTS_LIMIT,
};

/// Negation words that suggest contradiction.
const NEGATION_WORDS: &[&str] = &[
    "not", "no", "never", "don't", "doesn't", "didn't", "won't", "wouldn't",
    "can't", "cannot", "shouldn't", "isn't", "aren't", "wasn't", "weren't",
    "none", "nothing", "neither", "nor", "without", "lacks", "missing",
];

/// Opposite word pairs that suggest contradiction.
const OPPOSITE_PAIRS: &[(&str, &str)] = &[
    ("true", "false"),
    ("yes", "no"),
    ("enabled", "disabled"),
    ("enable", "disable"),
    ("active", "inactive"),
    ("allow", "deny"),
    ("allowed", "denied"),
    ("include", "exclude"),
    ("required", "optional"),
    ("public", "private"),
    ("async", "sync"),
    ("mutable", "immutable"),
    ("valid", "invalid"),
    ("success", "failure"),
    ("add", "remove"),
    ("create", "delete"),
    ("start", "stop"),
    ("open", "close"),
    ("on", "off"),
];

/// Check if two facts might be contradictory based on content analysis.
fn detect_contradiction(new_content: &str, existing_content: &str) -> Option<String> {
    let new_lower = new_content.to_lowercase();
    let existing_lower = existing_content.to_lowercase();

    // Check for negation patterns
    let new_has_negation = NEGATION_WORDS.iter().any(|w| new_lower.contains(w));
    let existing_has_negation = NEGATION_WORDS.iter().any(|w| existing_lower.contains(w));

    // If one has negation and the other doesn't, might be contradiction
    if new_has_negation != existing_has_negation {
        // Find significant shared words (excluding common words)
        let new_words: std::collections::HashSet<_> = new_lower
            .split_whitespace()
            .filter(|w| w.len() > 3)
            .collect();
        let existing_words: std::collections::HashSet<_> = existing_lower
            .split_whitespace()
            .filter(|w| w.len() > 3)
            .collect();

        let shared: Vec<&str> = new_words.intersection(&existing_words).copied().collect();
        if shared.len() >= 2 {
            return Some(format!(
                "Potential negation conflict: facts share terms ({}) but differ in assertion",
                shared.iter().take(3).copied().collect::<Vec<_>>().join(", ")
            ));
        }
    }

    // Check for opposite word pairs
    for (word_a, word_b) in OPPOSITE_PAIRS {
        let new_has_a = new_lower.contains(word_a);
        let new_has_b = new_lower.contains(word_b);
        let existing_has_a = existing_lower.contains(word_a);
        let existing_has_b = existing_lower.contains(word_b);

        // If one fact has word_a and the other has word_b, might be contradiction
        if (new_has_a && existing_has_b) || (new_has_b && existing_has_a) {
            return Some(format!(
                "Potential opposite values: '{}' vs '{}'",
                word_a, word_b
            ));
        }
    }

    None
}

/// Tool handler wrapping storage operations.
pub struct ToolHandler {
    storage: Storage,
    /// Session ID for this server instance (generated on startup).
    session_id: String,
}

impl ToolHandler {
    pub fn new(storage: Storage) -> Self {
        Self {
            storage,
            session_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    /// Get the current session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    // ========================================================================
    // Core Memory Tools
    // ========================================================================

    /// Remember a new fact.
    pub fn remember(&self, input: RememberInput) -> Result<RememberOutput> {
        // Validate content
        validate_content(&input.content)?;

        let mut fact = Fact::new(&input.content);

        // Auto-extract topics if not provided, then validate
        let topics = input.topics.unwrap_or_else(|| extract_topics(&input.content));
        validate_topics(&topics)?;
        fact.topics = topics;

        // Validate and set source if provided
        if let Some(source) = input.source {
            validate_source(&source)?;
            fact = fact.with_source(&source);
            // Compute hash for code sources
            if let Some(hash) = compute_source_hash(&source) {
                fact.source_content_hash = Some(hash);
            }
        }

        // Validate evidence if provided
        if let Some(ref evidence) = input.evidence {
            validate_evidence(evidence)?;
        }

        // Set optional fields
        if let Some(confidence) = input.confidence {
            fact.confidence = confidence.clamp(0.0, 1.0);
        }
        if let Some(category) = input.category {
            fact.category = category;
        }
        if let Some(importance) = input.importance {
            fact.importance = importance;
        }
        if let Some(certainty) = input.certainty {
            fact.certainty = certainty;
        }
        if let Some(scope) = input.scope {
            fact.scope = scope;
        }
        if let Some(source_type) = input.source_type {
            fact.source_type = source_type;
        }
        if let Some(evidence) = input.evidence {
            fact.evidence = evidence;
        }

        // Capture git commit, project path, and session ID
        fact.git_commit = get_git_commit();
        fact.project_path = get_project_root();
        fact.session_id = Some(self.session_id.clone());

        // Detect potential contradictions with existing facts
        let mut warnings = Vec::new();
        if !fact.topics.is_empty() {
            // Find facts with >50% topic overlap
            let overlapping_facts = self
                .storage
                .find_facts_by_topic_overlap(&fact.topics, 0.5, 20)?;

            for (existing_fact, _overlap) in overlapping_facts {
                // Skip self-comparison (shouldn't happen, but be safe)
                if existing_fact.id == fact.id {
                    continue;
                }

                // Check for contradiction signals
                if let Some(reason) = detect_contradiction(&input.content, &existing_fact.content) {
                    // Auto-create contradicts relation if both facts have high confidence
                    let relation_created = fact.confidence >= 0.7 && existing_fact.confidence >= 0.7;

                    warnings.push(ContradictionWarning {
                        existing_fact_id: existing_fact.id,
                        existing_fact_content: existing_fact.content.chars().take(100).collect::<String>()
                            + if existing_fact.content.len() > 100 { "..." } else { "" },
                        reason,
                        relation_created,
                    });
                }
            }
        }

        let id = fact.id;
        self.storage.insert_fact(&fact)?;

        // Now insert any contradiction relations
        for warning in &warnings {
            if warning.relation_created {
                let relation = Relation::new(id, warning.existing_fact_id, RelationType::Contradicts)
                    .with_metadata(&warning.reason);
                let _ = self.storage.insert_relation(&relation);
            }
        }

        let warning_count = warnings.len();
        Ok(RememberOutput {
            id,
            message: if warning_count > 0 {
                format!(
                    "Stored fact with {} topics. Warning: {} potential contradiction(s) detected",
                    fact.topics.len(),
                    warning_count
                )
            } else {
                format!("Stored fact with {} topics", fact.topics.len())
            },
            warnings,
        })
    }

    /// Recall facts matching a query.
    pub fn recall(&self, input: RecallInput) -> Result<RecallOutput> {
        // Validate query
        validate_query(&input.query)?;

        let filter = input.filter.unwrap_or_default();
        // Validate and cap the limit
        let limit = validate_limit(input.limit.unwrap_or(10));

        let facts = self.storage.search(&input.query, &filter, limit)?;

        // Mark each fact as accessed
        for fact in &facts {
            let _ = self.storage.mark_accessed(fact.id);
        }

        let count = facts.len();
        Ok(RecallOutput { facts, count })
    }

    /// Forget (delete) a fact.
    pub fn forget(&self, input: ForgetInput) -> Result<ForgetOutput> {
        let deleted = self.storage.delete_fact(input.fact_id)?;
        Ok(ForgetOutput {
            success: deleted,
            message: if deleted {
                "Fact deleted".to_string()
            } else {
                "Fact not found".to_string()
            },
        })
    }

    /// Remove a specific observation/evidence from a fact.
    pub fn forget_observation(&self, input: ForgetObservationInput) -> Result<ForgetOutput> {
        let Some(mut fact) = self.storage.get_fact(input.fact_id)? else {
            return Ok(ForgetOutput {
                success: false,
                message: "Fact not found".to_string(),
            });
        };

        let original_len = fact.evidence.len();
        fact.evidence.retain(|e| e != &input.observation);

        if fact.evidence.len() < original_len {
            self.storage.update_fact(&fact)?;
            Ok(ForgetOutput {
                success: true,
                message: "Observation removed".to_string(),
            })
        } else {
            Ok(ForgetOutput {
                success: false,
                message: "Observation not found in fact".to_string(),
            })
        }
    }

    // ========================================================================
    // Verification Tools
    // ========================================================================

    /// Verify a fact's source hasn't changed.
    pub fn verify(&self, input: VerifyInput) -> Result<VerifyOutput> {
        let Some(fact) = self.storage.get_fact(input.fact_id)? else {
            return Err(MemoryError::NotFound(input.fact_id));
        };

        let still_valid = verify_source(&fact)?;

        if still_valid {
            self.storage.mark_verified(fact.id)?;
            Ok(VerifyOutput {
                fact_id: fact.id,
                still_valid: true,
                message: "Source unchanged, fact verified".to_string(),
                new_hash: None,
            })
        } else {
            self.storage.mark_stale(fact.id, true)?;
            let new_hash = fact.source.as_ref().and_then(|s| compute_source_hash(s));
            Ok(VerifyOutput {
                fact_id: fact.id,
                still_valid: false,
                message: "Source has changed, fact marked stale".to_string(),
                new_hash,
            })
        }
    }

    /// Get facts that need verification.
    pub fn get_stale(&self, input: GetStaleInput) -> Result<GetStaleOutput> {
        let facts = self.storage.get_stale_facts(input.threshold_hours)?;
        let count = facts.len();
        Ok(GetStaleOutput { facts, count })
    }

    /// Refresh all facts with file sources.
    pub fn refresh_all(&self) -> Result<RefreshAllOutput> {
        let mut verified = 0;
        let mut stale = 0;
        let mut errors = Vec::new();

        // Cap to MAX_RESULTS_LIMIT for safety
        let all_facts = self.storage.list_facts(&FactFilter::default(), MAX_RESULTS_LIMIT)?;

        for fact in all_facts {
            if fact.source.is_some() && fact.source_content_hash.is_some() {
                match verify_source(&fact) {
                    Ok(true) => {
                        self.storage.mark_verified(fact.id)?;
                        verified += 1;
                    }
                    Ok(false) => {
                        self.storage.mark_stale(fact.id, true)?;
                        stale += 1;
                    }
                    Err(e) => {
                        errors.push(format!("{}: {}", fact.id, e));
                    }
                }
            }
        }

        Ok(RefreshAllOutput {
            verified,
            stale,
            errors,
        })
    }

    // ========================================================================
    // Relationship Tools
    // ========================================================================

    /// Link two facts.
    pub fn link(&self, input: LinkInput) -> Result<LinkOutput> {
        let relation = Relation::new(input.fact_a, input.fact_b, input.relation_type)
            .with_metadata(input.metadata.unwrap_or_default());

        self.storage.insert_relation(&relation)?;

        Ok(LinkOutput {
            success: true,
            message: format!(
                "Linked {} -> {} ({:?})",
                input.fact_a, input.fact_b, input.relation_type
            ),
        })
    }

    /// Unlink two facts.
    pub fn unlink(&self, input: UnlinkInput) -> Result<LinkOutput> {
        let deleted = self
            .storage
            .delete_relation(input.fact_a, input.fact_b, input.relation_type)?;

        Ok(LinkOutput {
            success: deleted,
            message: if deleted {
                "Relation removed".to_string()
            } else {
                "Relation not found".to_string()
            },
        })
    }

    /// Get related facts.
    pub fn get_related(&self, input: GetRelatedInput) -> Result<GetRelatedOutput> {
        let relations = self.storage.get_relations(input.fact_id)?;

        let mut related_facts = Vec::new();
        for rel in &relations {
            let related_id = if rel.from_id == input.fact_id {
                rel.to_id
            } else {
                rel.from_id
            };

            if let Some(fact) = self.storage.get_fact(related_id)? {
                related_facts.push(RelatedFact {
                    fact,
                    relation_type: rel.relation_type,
                    direction: if rel.from_id == input.fact_id {
                        "outgoing".to_string()
                    } else {
                        "incoming".to_string()
                    },
                });
            }
        }

        Ok(GetRelatedOutput {
            source_id: input.fact_id,
            related: related_facts,
        })
    }

    /// Find contradictions.
    pub fn find_contradictions(&self) -> Result<FindContradictionsOutput> {
        let contradictions = self.storage.find_contradictions()?;
        Ok(FindContradictionsOutput {
            contradictions: contradictions
                .into_iter()
                .map(|(a, b, reason)| ContradictionPair {
                    fact_a: a,
                    fact_b: b,
                    reason,
                })
                .collect(),
        })
    }

    // ========================================================================
    // Exploration Tools
    // ========================================================================

    /// List all topics with counts.
    pub fn list_topics(&self) -> Result<ListTopicsOutput> {
        let topics = self.storage.list_topics()?;
        Ok(ListTopicsOutput {
            topics: topics
                .into_iter()
                .map(|(topic, count)| TopicInfo { topic, count })
                .collect(),
        })
    }

    /// Summarize facts on a topic.
    pub fn summarize(&self, input: SummarizeInput) -> Result<SummarizeOutput> {
        let filter = FactFilter {
            topics: Some(vec![input.topic.clone()]),
            ..Default::default()
        };

        let facts = self.storage.search("", &filter, input.limit.unwrap_or(20))?;

        Ok(SummarizeOutput {
            topic: input.topic,
            fact_count: facts.len(),
            facts,
        })
    }

    /// Get storage stats.
    pub fn stats(&self) -> Result<StatsOutput> {
        let count = self.storage.count_facts()?;
        let topics = self.storage.list_topics()?;
        let stale = self.storage.get_stale_facts(None)?;

        Ok(StatsOutput {
            total_facts: count,
            total_topics: topics.len(),
            stale_facts: stale.len(),
        })
    }

    // ========================================================================
    // Memory Maintenance Tools
    // ========================================================================

    /// Apply time-based confidence decay to old, unused facts.
    pub fn decay(&self, input: DecayInput) -> Result<DecayOutput> {
        let threshold_days = input.threshold_days.unwrap_or(30);
        let decay_factor = input.decay_factor.unwrap_or(0.9).clamp(0.1, 1.0);

        let (facts_affected, total_reduction) =
            self.storage.apply_decay(threshold_days, decay_factor)?;

        Ok(DecayOutput {
            facts_affected,
            total_confidence_reduction: total_reduction,
            message: format!(
                "Applied {}% decay to {} facts not accessed in {} days",
                ((1.0 - decay_factor) * 100.0) as i32,
                facts_affected,
                threshold_days
            ),
        })
    }

    /// Prune (archive or delete) old, unused, low-confidence facts.
    pub fn prune(&self, input: PruneInput) -> Result<PruneOutput> {
        let days_unused = input.days_unused.unwrap_or(90);
        let min_confidence = input.min_confidence.unwrap_or(0.5).clamp(0.0, 1.0);
        let archive = input.archive.unwrap_or(true);

        let fact_ids = self.storage.prune_facts(days_unused, min_confidence, archive)?;
        let count = fact_ids.len();

        Ok(PruneOutput {
            facts_pruned: count,
            fact_ids,
            archived: archive,
            message: format!(
                "{} {} facts (unused for {} days, confidence < {})",
                if archive { "Archived" } else { "Deleted" },
                count,
                days_unused,
                min_confidence
            ),
        })
    }

    /// Find similar facts based on topic overlap for potential consolidation.
    pub fn consolidate(&self, input: ConsolidateInput) -> Result<ConsolidateOutput> {
        let threshold = input.similarity_threshold.unwrap_or(0.5).clamp(0.0, 1.0);

        let pairs = self.storage.find_similar_facts(threshold)?;
        let count = pairs.len();

        Ok(ConsolidateOutput {
            similar_pairs: pairs
                .into_iter()
                .map(|(a, b, sim)| SimilarFactPair {
                    fact_a: a,
                    fact_b: b,
                    similarity: sim,
                })
                .collect(),
            count,
            message: format!("Found {} similar fact pairs (threshold: {})", count, threshold),
        })
    }

    /// Archive a fact (soft-delete).
    pub fn archive(&self, input: ArchiveInput) -> Result<ForgetOutput> {
        let archived = self.storage.archive_fact(input.fact_id)?;
        Ok(ForgetOutput {
            success: archived,
            message: if archived {
                "Fact archived".to_string()
            } else {
                "Fact not found or already archived".to_string()
            },
        })
    }

    /// Unarchive a fact.
    pub fn unarchive(&self, input: ArchiveInput) -> Result<ForgetOutput> {
        let unarchived = self.storage.unarchive_fact(input.fact_id)?;
        Ok(ForgetOutput {
            success: unarchived,
            message: if unarchived {
                "Fact restored from archive".to_string()
            } else {
                "Fact not found or not archived".to_string()
            },
        })
    }

    /// Get archived facts.
    pub fn get_archived(&self, input: GetArchivedInput) -> Result<GetArchivedOutput> {
        let limit = input.limit.unwrap_or(50).min(MAX_RESULTS_LIMIT);
        let facts = self.storage.get_archived_facts(limit)?;
        let count = facts.len();
        Ok(GetArchivedOutput { facts, count })
    }

    // ========================================================================
    // Session Tools
    // ========================================================================

    /// Get facts from the current or specified session.
    pub fn get_session_facts(&self, input: GetSessionFactsInput) -> Result<GetSessionFactsOutput> {
        let session_id = input.session_id.unwrap_or_else(|| self.session_id.clone());
        let limit = input.limit.unwrap_or(50).min(MAX_RESULTS_LIMIT);

        let filter = FactFilter {
            session_id: Some(session_id.clone()),
            all_projects: true, // Session facts can be from any project
            ..Default::default()
        };

        let facts = self.storage.search("", &filter, limit)?;
        let count = facts.len();

        Ok(GetSessionFactsOutput {
            session_id,
            facts,
            count,
        })
    }

    // ========================================================================
    // History Tools
    // ========================================================================

    /// Get the history of changes for a fact.
    pub fn get_fact_history(&self, input: GetFactHistoryInput) -> Result<GetFactHistoryOutput> {
        let current = self.storage.get_fact(input.fact_id)?;
        let history = self.storage.get_fact_history(input.fact_id)?;
        let version_count = history.len();

        Ok(GetFactHistoryOutput {
            fact_id: input.fact_id,
            current,
            history,
            version_count,
        })
    }

    // ========================================================================
    // Merge Tools
    // ========================================================================

    // ========================================================================
    // Category Summary Tools
    // ========================================================================

    /// Get summaries of facts grouped by category.
    pub fn get_category_summary(
        &self,
        input: GetCategorySummaryInput,
    ) -> Result<GetCategorySummaryOutput> {
        let limit = input.limit_per_category.unwrap_or(10).min(MAX_RESULTS_LIMIT);
        let category_data = self.storage.get_facts_by_category(input.category, limit)?;

        let summaries: Vec<CategorySummary> = category_data
            .into_iter()
            .map(|(category, facts, top_topics)| {
                let fact_count = facts.len();
                CategorySummary {
                    category,
                    fact_count,
                    top_topics,
                    sample_facts: facts,
                }
            })
            .collect();

        let total = summaries.len();
        Ok(GetCategorySummaryOutput {
            summaries,
            total_categories: total,
        })
    }

    // ========================================================================
    // Merge Tools
    // ========================================================================

    /// Merge two similar facts into one.
    ///
    /// The merged fact:
    /// - Uses the provided merged_content or fact_a's content
    /// - Takes the higher confidence
    /// - Combines topics and evidence from both
    /// - Links to originals via supersedes relations
    /// - Archives the original facts
    pub fn merge_facts(&self, input: MergeFactsInput) -> Result<MergeFactsOutput> {
        // Get both facts
        let fact_a = self
            .storage
            .get_fact(input.fact_a)?
            .ok_or_else(|| MemoryError::NotFound(input.fact_a))?;
        let fact_b = self
            .storage
            .get_fact(input.fact_b)?
            .ok_or_else(|| MemoryError::NotFound(input.fact_b))?;

        // Create merged fact
        let merged_content = input.merged_content.unwrap_or_else(|| {
            // If no merged content provided, use fact_a's content with a note
            format!("{} (merged from {} and {})", fact_a.content, input.fact_a, input.fact_b)
        });

        let mut merged_fact = Fact::new(&merged_content);

        // Take higher confidence
        merged_fact.confidence = fact_a.confidence.max(fact_b.confidence);

        // Take higher importance
        merged_fact.importance = if fact_a.importance >= fact_b.importance {
            fact_a.importance
        } else {
            fact_b.importance
        };

        // Combine topics (deduplicated)
        let mut topics: std::collections::HashSet<String> =
            fact_a.topics.into_iter().collect();
        topics.extend(fact_b.topics);
        merged_fact.topics = topics.into_iter().collect();

        // Combine evidence
        let mut evidence: Vec<String> = fact_a.evidence;
        evidence.extend(fact_b.evidence);
        // Deduplicate evidence
        evidence.sort();
        evidence.dedup();
        merged_fact.evidence = evidence;

        // Take the more specific category if different
        merged_fact.category = fact_a.category;

        // Keep project path from fact_a
        merged_fact.project_path = fact_a.project_path;

        // Set session ID
        merged_fact.session_id = Some(self.session_id.clone());

        // Insert the merged fact
        let merged_id = merged_fact.id;
        self.storage.insert_fact(&merged_fact)?;

        // Create supersedes relations
        let rel_a = Relation::new(merged_id, input.fact_a, RelationType::Supersedes)
            .with_metadata("Merged from original fact");
        self.storage.insert_relation(&rel_a)?;

        let rel_b = Relation::new(merged_id, input.fact_b, RelationType::Supersedes)
            .with_metadata("Merged from original fact");
        self.storage.insert_relation(&rel_b)?;

        // Archive the original facts
        self.storage.archive_fact(input.fact_a)?;
        self.storage.archive_fact(input.fact_b)?;

        Ok(MergeFactsOutput {
            merged_id,
            archived_ids: vec![input.fact_a, input.fact_b],
            message: format!(
                "Merged facts {} and {} into {}. Original facts archived.",
                input.fact_a, input.fact_b, merged_id
            ),
        })
    }
}
