# Context-Memory Enhancements Plan

Based on Rohit's article "How to build an agent that never forgets"

## Overview

Enhance context-memory with three key features from the article:
1. **Memory Maintenance & Decay** - Tools for pruning, decaying, and consolidating facts
2. **Time-Weighted Retrieval** - Search results factor in recency
3. **Archived Facts Support** - Soft-delete instead of hard-delete

## Implementation Steps

### Step 1: Add `decay` MCP Tool
**Files:** `src/storage.rs`, `src/tools/mod.rs`, `src/tools/types.rs`, `src/server.rs`

Add a tool that applies time-based confidence decay to facts:
- Facts lose confidence over time if not accessed
- Formula: `new_confidence = old_confidence * (1 / (1 + (days_since_access / 30)))`
- Option to decay only facts not accessed in N days

Storage method:
```rust
fn apply_decay(&self, days_threshold: i64, decay_factor: f32) -> Result<DecayStats>
```

MCP Tool: `decay`
- Parameters: `threshold_days: Option<i64>`, `decay_factor: Option<f32>`
- Returns: count of facts decayed, average confidence reduction

### Step 2: Add `consolidate` MCP Tool
**Files:** `src/storage.rs`, `src/tools/mod.rs`, `src/tools/types.rs`, `src/server.rs`

Add a tool that finds and reports potential duplicate/redundant facts:
- Use FTS5 to find facts with similar content
- Group facts by topic overlap
- Return pairs with similarity scores for manual review

Storage method:
```rust
fn find_similar_facts(&self, threshold: f32) -> Result<Vec<SimilarFactPair>>
```

MCP Tool: `consolidate`
- Parameters: `similarity_threshold: Option<f32>`, `dry_run: Option<bool>`
- Returns: list of similar fact pairs with scores

### Step 3: Add `prune` MCP Tool
**Files:** `src/storage.rs`, `src/tools/mod.rs`, `src/tools/types.rs`, `src/server.rs`

Add a tool that archives or deletes old, unused, low-confidence facts:
- Criteria: not accessed in N days AND confidence below threshold
- Option to archive (soft-delete) instead of delete

Storage methods:
```rust
fn archive_fact(&self, id: Uuid) -> Result<()>
fn prune_facts(&self, days: i64, min_confidence: f32, archive: bool) -> Result<PruneStats>
```

MCP Tool: `prune`
- Parameters: `days_unused: Option<i64>`, `min_confidence: Option<f32>`, `archive: Option<bool>`
- Returns: count pruned, list of fact IDs affected

### Step 4: Add Time-Weighted Retrieval
**Files:** `src/storage.rs`

Modify the search method to factor recency into ranking:
- Calculate time decay for each result
- Combine with existing relevance/importance scoring
- Formula: `final_score = relevance * importance_weight * time_decay`
- Where: `time_decay = 1.0 / (1.0 + (days_since_access / 30))`

### Step 5: Add Schema Migration for Archived Flag
**Files:** `src/storage.rs`

Add `archived` column to facts table:
- Boolean flag, default false
- Archived facts excluded from normal queries
- Add migration logic in `init_schema`

### Step 6: Update README
**Files:** `README.md`

Document new tools and features.

## Parallel Batches

### Batch 1 (parallel) - New Tools
| Task | Files | Dependencies |
|------|-------|--------------|
| decay tool | storage.rs, tools/mod.rs, tools/types.rs, server.rs | None |
| consolidate tool | storage.rs, tools/mod.rs, tools/types.rs, server.rs | None |
| prune tool | storage.rs, tools/mod.rs, tools/types.rs, server.rs | None |

**Note:** These touch the same files but add non-overlapping code (new functions/tools).

### Batch 2 (sequential)
| Task | Files | Dependencies |
|------|-------|--------------|
| Schema migration (archived) | storage.rs | Batch 1 complete |
| Time-weighted retrieval | storage.rs | Schema ready |

### Batch 3 (sequential)
| Task | Files | Dependencies |
|------|-------|--------------|
| Integration tests | tests/ | All features |
| README update | README.md | All features |

## Testing Strategy

Each new tool needs:
1. Unit tests in `storage.rs` tests module
2. Integration test via MCP tool invocation

## Rollback Plan

All changes are additive. Existing tools unchanged. Schema migration adds nullable column.
