//! MCP Tool implementations.

mod types;

pub use types::*;

use crate::error::{MemoryError, Result};
use crate::storage::Storage;
use crate::types::{Fact, FactFilter, Relation};
use crate::utils::{compute_source_hash, extract_topics, get_git_commit, verify_source};
use crate::validation::{
    validate_content, validate_evidence, validate_limit, validate_query, validate_source,
    validate_topics, MAX_RESULTS_LIMIT,
};

/// Tool handler wrapping storage operations.
pub struct ToolHandler {
    storage: Storage,
}

impl ToolHandler {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
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

        // Capture git commit
        fact.git_commit = get_git_commit();

        let id = fact.id;
        self.storage.insert_fact(&fact)?;

        Ok(RememberOutput {
            id,
            message: format!("Stored fact with {} topics", fact.topics.len()),
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
}
