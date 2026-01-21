//! Enum types used throughout the context-memory system.

use serde::{Deserialize, Serialize};

/// Type of source from which a fact was derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    Code,
    Conversation,
    #[default]
    Manual,
    Inferred,
}

/// Category of a fact for organizational purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Architecture,
    Decision,
    Pattern,
    Convention,
    Bug,
    Todo,
    Dependency,
    Preference,
    #[default]
    Context,
}

/// Importance level of a fact.
///
/// Ordered from highest to lowest: Critical > High > Normal > Low
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Importance {
    Low = 0,
    #[default]
    Normal = 1,
    High = 2,
    Critical = 3,
}

/// Certainty level of a fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Certainty {
    Definite,
    #[default]
    Likely,
    Uncertain,
    Speculative,
}

/// Scope of a fact's applicability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Global,
    #[default]
    Project,
    Branch,
    Task,
}

/// Type of relationship between two facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
    DependsOn,
    Contradicts,
    Elaborates,
    RelatedTo,
    PartOf,
    Supersedes,
}
