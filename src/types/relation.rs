//! Relation data structure - connections between facts.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::enums::RelationType;

/// A relationship between two facts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub from_id: Uuid,
    pub to_id: Uuid,
    pub relation_type: RelationType,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
}

impl Relation {
    /// Create a new relation between two facts.
    pub fn new(from_id: Uuid, to_id: Uuid, relation_type: RelationType) -> Self {
        Self {
            from_id,
            to_id,
            relation_type,
            created_at: Utc::now(),
            metadata: None,
        }
    }

    /// Add metadata to this relation.
    pub fn with_metadata(mut self, metadata: impl Into<String>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }
}
