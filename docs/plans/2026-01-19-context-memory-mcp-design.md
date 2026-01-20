# Context Memory MCP - Design Document

**Date**: 2026-01-19
**Status**: Approved for implementation

## Overview

A Rust MCP server providing persistent, context-rot-resistant memory for Claude Code. Unlike existing memory MCPs, this focuses on source provenance, staleness detection, and contradiction awareness.

## Goals

1. **Anti-context-rot**: Facts stored externally, verified against sources
2. **Efficient context usage**: Return minimal, structured data
3. **Fully local**: SQLite storage, no external APIs
4. **Source provenance**: Every fact tracks where it came from

## Data Model

```rust
struct Fact {
    id: Uuid,
    content: String,

    // Source provenance
    source: Option<String>,              // "src/auth.rs:42"
    source_type: SourceType,             // Code | Conversation | Manual
    source_content_hash: Option<String>, // Detect changes
    git_commit: Option<String>,

    // Confidence & lifecycle
    confidence: f32,                     // 0.0-1.0
    certainty: Certainty,                // Definite | Likely | Uncertain
    created_at: DateTime,
    last_verified: DateTime,
    stale: bool,

    // Categorization
    topics: Vec<String>,
    category: Category,                  // Architecture | Decision | Pattern | etc.
    importance: Importance,              // Critical | High | Normal | Low
    scope: Scope,                        // Global | Project | Branch | Task

    // Provenance chain
    derived_from: Option<Uuid>,
    supersedes: Option<Uuid>,
    evidence: Vec<String>,

    // Usage tracking
    access_count: u32,
    last_accessed: Option<DateTime>,
}

struct Relation {
    from_id: Uuid,
    to_id: Uuid,
    relation_type: RelationType,         // DependsOn | Contradicts | Elaborates | RelatedTo | PartOf
}

enum SourceType { Code, Conversation, Manual, Inferred }
enum Category { Architecture, Decision, Pattern, Convention, Bug, Todo, Dependency }
enum Importance { Critical, High, Normal, Low }
enum Certainty { Definite, Likely, Uncertain, Speculative }
enum Scope { Global, Project, Branch, Task }
enum RelationType { DependsOn, Contradicts, Elaborates, RelatedTo, PartOf }
```

## Storage

- **Backend**: SQLite with FTS5 for full-text search
- **Location**: `~/.claude/context-memory/memory.db`
- **Future**: Add sqlite-vec for semantic search

## MCP Tools

### Core Memory
- `remember(content, source?, topics?, confidence?)` - Store fact with auto-population
- `recall(query, limit?, filters?)` - Search by keyword/topic
- `forget(fact_id)` - Remove fact
- `forget_observation(fact_id, observation)` - Remove specific observation

### Relationships
- `link(fact_a, fact_b, relation_type)` - Connect facts
- `unlink(fact_a, fact_b, relation_type)` - Remove connection
- `get_related(fact_id)` - Find connected facts
- `find_contradictions()` - Detect conflicting facts

### Verification (Unique)
- `verify(fact_id)` - Re-check source file, update staleness
- `get_stale(threshold_hours?)` - List facts needing verification
- `refresh_all()` - Batch verify all facts with file sources

### Exploration
- `list_topics()` - All known topics with counts
- `summarize(topic)` - Overview of facts on topic
- `get_graph(filter?)` - Full or filtered knowledge structure

### Session
- `checkpoint(name)` - Snapshot current state
- `restore(checkpoint)` - Roll back to checkpoint
- `start_task(description)` - Scope subsequent facts to task
- `end_task()` - Close task scope

## Key Differentiators

1. **Source provenance** - Know where every fact came from
2. **Staleness detection** - Automatically detect when sources change
3. **Contradiction awareness** - Flag conflicting facts
4. **Minimal responses** - Return structured data, not raw content

## Implementation Phases

1. **Phase 1**: Core model + SQLite + remember/recall/forget
2. **Phase 2**: Verification tools (verify, get_stale)
3. **Phase 3**: Relationships (link, get_related, find_contradictions)
4. **Phase 4**: Session management (checkpoint, task scoping)
5. **Phase 5**: Exploration (summarize, get_graph)
