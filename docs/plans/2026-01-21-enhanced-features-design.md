# Enhanced Features Design

## Phase 1: Quick Wins

### 1.1 Confidence Boost on Access
**Problem:** Facts only decay, never strengthen. Frequently-used facts should gain confidence.

**Solution:** When `mark_accessed()` is called, also boost confidence slightly.
- Formula: `new_confidence = min(1.0, old_confidence + boost_factor)`
- Default boost: 0.02 (2% per access)
- Cap at 1.0

**Files:** `src/storage.rs`

### 1.2 Canonical Project Paths
**Problem:** Symlinks and different path representations cause project isolation issues.

**Solution:** Canonicalize all paths before storing/comparing.
- Use `std::fs::canonicalize()` on project paths
- Normalize before insert and before search filter

**Files:** `src/utils.rs`, `src/storage.rs`

---

## Phase 2: Core Features

### 2.1 Session/Conversation Tracking
**Problem:** Can't query "what did I learn this session?"

**Solution:** Add `session_id` field to facts.
- Generate session ID on server startup (UUID)
- Store with each fact
- Add filter option for session-specific queries
- Add `get_session_facts()` tool

**Schema changes:**
```sql
ALTER TABLE facts ADD COLUMN session_id TEXT;
CREATE INDEX idx_facts_session ON facts(session_id);
```

**Files:** `src/types/fact.rs`, `src/storage.rs`, `src/server.rs`, `src/tools/`

### 2.2 Contradiction Detection on Write
**Problem:** New facts can silently conflict with existing ones.

**Solution:** Before storing a new fact, check for potential contradictions.
- Search for facts with high topic overlap (>50%)
- Use simple heuristics: negation words, opposite values
- Return warnings but still store (don't block)
- Auto-create `contradicts` relation if confidence > threshold

**Files:** `src/storage.rs`, `src/tools/mod.rs`

### 2.3 Fact History/Versioning
**Problem:** Updates overwrite history, losing knowledge evolution.

**Solution:** Create `fact_history` table to track versions.
- Before update, snapshot current state
- Track: fact_id, version, content, confidence, updated_at, change_reason
- Add `get_fact_history()` tool

**Schema:**
```sql
CREATE TABLE fact_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    fact_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    content TEXT NOT NULL,
    confidence REAL NOT NULL,
    changed_at TEXT NOT NULL,
    change_reason TEXT,
    FOREIGN KEY (fact_id) REFERENCES facts(id)
);
```

**Files:** `src/storage.rs`, `src/tools/`

---

## Phase 3: Advanced Features

### 3.1 Auto-Maintenance Hooks
**Problem:** User must manually run decay/prune.

**Solution:** Run maintenance on server startup.
- On startup: run light decay (7 days, 0.95 factor)
- Configurable via environment variables
- Log maintenance results

**Files:** `src/server.rs`, `src/storage.rs`

### 3.2 Auto-Merge Consolidate
**Problem:** Consolidate only finds similar facts, doesn't merge.

**Solution:** Add `merge_facts()` function.
- Combine evidence arrays
- Keep higher confidence
- Link merged fact to originals via `supersedes`
- Archive the originals

**Files:** `src/storage.rs`, `src/tools/mod.rs`

### 3.3 Category Summaries
**Problem:** Flat fact storage, no hierarchical organization.

**Solution:** Add `category_summaries` table with auto-generated summaries.
- Group facts by category + topics
- Generate summary text (concatenate key facts)
- Update on fact insert/update
- Add `get_category_summary()` tool

**Schema:**
```sql
CREATE TABLE category_summaries (
    id TEXT PRIMARY KEY,
    category TEXT NOT NULL,
    topic TEXT NOT NULL,
    summary TEXT NOT NULL,
    fact_count INTEGER NOT NULL,
    last_updated TEXT NOT NULL,
    UNIQUE(category, topic)
);
```

**Files:** `src/storage.rs`, `src/tools/`
