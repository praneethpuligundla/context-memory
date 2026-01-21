# Context Memory

Context-rot-resistant memory MCP server for Claude Code. Gives Claude persistent memory that survives across sessions, with automatic staleness detection when source code changes.

## Why Context Memory?

Claude Code forgets everything between sessions. Context Memory solves this by:

1. **Persistent Storage** - Facts survive across sessions in SQLite
2. **Source Tracking** - Link facts to source files; detect when code changes invalidate them
3. **Smart Retrieval** - Time-weighted search ranks recent, relevant facts higher
4. **Memory Hygiene** - Decay, prune, and consolidate to prevent memory rot
5. **Session Tracking** - Know what was learned in the current conversation
6. **Contradiction Detection** - Automatic warnings when new facts contradict existing ones

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Claude Code                               │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ MCP Protocol (stdio)
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Context Memory Server                         │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │ Tool Router │──│Tool Handler │──│   Storage Layer         │  │
│  │  (rmcp)     │  │             │  │                         │  │
│  └─────────────┘  └─────────────┘  │  ┌─────────────────┐    │  │
│                                     │  │  SQLite + FTS5  │    │  │
│  Tools:                            │  │                 │    │  │
│  • remember/recall/forget          │  │  • facts        │    │  │
│  • verify/get_stale/refresh_all    │  │  • relations    │    │  │
│  • link/unlink/get_related         │  │  • topics       │    │  │
│  • decay/prune/consolidate         │  │  • evidence     │    │  │
│  • archive/unarchive               │  │  • history      │    │  │
│  • get_session_facts               │  └─────────────────┘    │  │
│  • merge_facts                     └─────────────────────────┘  │
│  • get_category_summary                                         │
│  • get_fact_history                                             │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
                    ~/.claude/context-memory/memory.db
```

### Data Model

```
Fact
├── id (UUID)
├── content (the fact itself)
├── project_path (git root or cwd - for project isolation)
├── session_id (conversation tracking - set automatically)
├── source (file:line reference)
├── source_content_hash (for staleness detection)
├── confidence (0.0-1.0, decays over time, boosts on access)
├── importance (critical/high/normal/low)
├── certainty (definite/likely/uncertain/speculative)
├── category (architecture/decision/pattern/bug/todo/...)
├── scope (global/project/branch/task)
├── topics[] (tags for categorization)
├── evidence[] (supporting observations)
├── access_count (usage tracking)
├── last_accessed (for time-weighted retrieval)
├── stale (true if source has changed)
└── archived (soft-delete flag)

Fact History (versioning)
├── fact_id → version
├── content (snapshot)
├── confidence (snapshot)
├── changed_at
└── change_reason

Relations
├── from_id → to_id
├── type (depends_on/contradicts/elaborates/related_to/part_of/supersedes)
└── metadata
```

### Project Isolation

Facts are automatically scoped to the project where they were created:

- **Auto-detection** - When storing a fact, the git root (or cwd) is captured
- **Auto-filtering** - Searches return only facts from the current project by default
- **Cross-project queries** - Use `all_projects: true` to search across all projects
- **Global facts** - Facts with no project_path are returned in all searches

### Session Tracking

Each server instance gets a unique session ID:

- **Auto-capture** - All facts stored include the current session ID
- **Query by session** - Use `get_session_facts()` to see what was learned this session
- **Filter by session** - Use `session_id` filter in recall to query specific sessions

### Contradiction Detection

When storing new facts, the system automatically checks for contradictions:

- **Topic overlap analysis** - Finds existing facts with >50% topic overlap
- **Content analysis** - Detects negation words and opposite value pairs
- **Automatic warnings** - Returns warnings but still stores the fact
- **Auto-linking** - Creates `contradicts` relations for high-confidence contradictions

## How It Works

### 1. Storing Facts

When Claude learns something worth remembering:

```
remember(
  content: "Auth module uses JWT with RS256 signing",
  source: "src/auth/jwt.rs:42",
  topics: ["auth", "jwt", "security"],
  category: "architecture",
  importance: "high"
)
```

The system:
- Generates a UUID for the fact
- Computes SHA256 hash of the source file content
- Auto-extracts additional topics from content
- Checks for potential contradictions with existing facts
- Stores in SQLite with FTS5 indexing
- Automatically captures project path and session ID

### 2. Retrieving Facts (Time-Weighted)

```
recall(query: "authentication")
```

Search uses time-weighted scoring:

```
score = confidence × importance_weight × time_decay

where:
  importance_weight = { critical: 4, high: 2, normal: 1, low: 0.5 }
  time_decay = 1 / (1 + days_since_access / 30)
```

Recent, important, high-confidence facts rank highest.

**Confidence boosting**: Each time a fact is accessed, its confidence increases by 2% (capped at 1.0), reinforcing frequently-used knowledge.

### 3. Staleness Detection

When a fact has a source reference (e.g., `src/auth/jwt.rs:42`):

```
verify(fact_id: "abc-123")
```

The system:
1. Reads the current file content
2. Computes new SHA256 hash
3. Compares with stored hash
4. Marks fact as `stale: true` if changed

### 4. Memory Maintenance

**Auto-maintenance on startup**: The server automatically runs light decay when starting:
- Default: 5% decay for facts not accessed in 7 days
- Configurable via environment variables

**Decay** - Reduce confidence of unused facts:
```
decay(threshold_days: 30, decay_factor: 0.9)
// Facts not accessed in 30 days lose 10% confidence
```

**Prune** - Remove low-value facts:
```
prune(days_unused: 90, min_confidence: 0.5, archive: true)
// Archive facts unused for 90 days with confidence < 0.5
```

**Consolidate** - Find duplicates:
```
consolidate(similarity_threshold: 0.5)
// Find fact pairs with >50% topic overlap (Jaccard similarity)
```

**Merge** - Combine similar facts:
```
merge_facts(fact_a: "uuid-1", fact_b: "uuid-2", merged_content: "Combined fact")
// Creates new fact, archives originals, links via supersedes relation
```

## Installation

### Prerequisites

- Rust 1.70+ (for building)
- Claude Code CLI

### Quick Start

```bash
# Clone and build
git clone https://github.com/your-repo/context-memory
cd context-memory
cargo build --release

# Install as Claude Code plugin
mkdir -p ~/.claude/plugins
ln -sf "$(pwd)/plugin" ~/.claude/plugins/context-memory

# Enable the plugin
# Add to ~/.claude/settings.json:
# {
#   "enabledPlugins": {
#     "context-memory@local": true
#   }
# }

# Restart Claude Code
```

### Alternative: Standalone MCP Server

If you prefer not to use the plugin system:

```bash
# Add to ~/.claude/.mcp.json
{
  "mcpServers": {
    "context-memory": {
      "command": "/absolute/path/to/context-memory/target/release/context-memory",
      "args": []
    }
  }
}
```

### Verify Installation

Start Claude Code and run:
```
> What memory tools do you have?

Claude should list: remember, recall, forget, verify, decay, prune, etc.
```

## MCP Tools Reference

### Core Memory

| Tool | Description | Key Parameters |
|------|-------------|----------------|
| `remember` | Store a fact | `content`, `source?`, `topics?`, `category?`, `importance?` |
| `recall` | Search facts | `query`, `all_projects?`, `session_id?`, `topics?`, `category?`, `min_confidence?`, `limit?` |
| `forget` | Delete a fact | `fact_id` |
| `forget_observation` | Remove evidence | `fact_id`, `observation` |

### Verification

| Tool | Description | Key Parameters |
|------|-------------|----------------|
| `verify` | Check if source changed | `fact_id` |
| `get_stale` | List stale facts | `threshold_hours?` |
| `refresh_all` | Batch verify all | - |

### Relationships

| Tool | Description | Key Parameters |
|------|-------------|----------------|
| `link` | Connect facts | `fact_a`, `fact_b`, `relation_type`, `metadata?` |
| `unlink` | Remove connection | `fact_a`, `fact_b`, `relation_type` |
| `get_related` | Find connected facts | `fact_id` |
| `find_contradictions` | Detect conflicts | - |

### Session & History

| Tool | Description | Key Parameters |
|------|-------------|----------------|
| `get_session_facts` | Facts from current/specific session | `session_id?`, `limit?` |
| `get_fact_history` | Version history of a fact | `fact_id` |

### Exploration

| Tool | Description | Key Parameters |
|------|-------------|----------------|
| `list_topics` | List all topics | - |
| `summarize` | Facts on a topic | `topic`, `limit?` |
| `get_category_summary` | Facts grouped by category | `category?`, `limit_per_category?` |
| `stats` | Memory statistics | - |

### Memory Maintenance

| Tool | Description | Key Parameters |
|------|-------------|----------------|
| `decay` | Reduce old confidence | `threshold_days?` (30), `decay_factor?` (0.9) |
| `prune` | Remove unused facts | `days_unused?` (90), `min_confidence?` (0.5), `archive?` (true) |
| `consolidate` | Find similar facts | `similarity_threshold?` (0.5) |
| `merge_facts` | Combine two facts | `fact_a`, `fact_b`, `merged_content?` |
| `archive` | Soft-delete | `fact_id` |
| `unarchive` | Restore archived | `fact_id` |
| `get_archived` | List archived | `limit?` |

## Usage Examples

### Basic Workflow

```
Claude: I'm learning about this codebase. Let me remember key findings.

> remember(
    content: "Database uses PostgreSQL with pgvector for embeddings",
    source: "docker-compose.yml:15",
    topics: ["database", "postgres", "embeddings"],
    category: "architecture",
    importance: "high"
  )

> remember(
    content: "API rate limit is 100 requests per minute per user",
    source: "src/middleware/rate_limit.rs:23",
    topics: ["api", "rate-limiting", "security"],
    category: "convention"
  )

[Later, in a new session...]

Claude: What do I know about the database?

> recall(query: "database", topics: ["database"])

[Returns: "Database uses PostgreSQL with pgvector for embeddings"]
```

### Session Tracking

```
Claude: What have I learned in this session?

> get_session_facts()

{
  "session_id": "abc-123-...",
  "facts": [...all facts from current session...],
  "count": 5
}
```

### Contradiction Detection

```
> remember(content: "API uses OAuth2 for authentication")
> remember(content: "API does not use OAuth2")

{
  "id": "...",
  "message": "Stored fact with 2 topics. Warning: 1 potential contradiction(s) detected",
  "warnings": [
    {
      "existing_fact_id": "...",
      "existing_fact_content": "API uses OAuth2 for authentication",
      "reason": "Potential negation conflict: facts share terms (api, oauth2) but differ in assertion",
      "relation_created": true
    }
  ]
}
```

### Fact History

```
Claude: What were the previous versions of this fact?

> get_fact_history(fact_id: "abc-123")

{
  "fact_id": "abc-123",
  "current": {...current fact...},
  "history": [
    {"version": 2, "content": "Previous content", "confidence": 0.8, "changed_at": "..."},
    {"version": 1, "content": "Original content", "confidence": 0.9, "changed_at": "..."}
  ]
}
```

### Category Summaries

```
Claude: Show me all my architecture decisions.

> get_category_summary(category: "architecture")

{
  "summaries": [{
    "category": "architecture",
    "fact_count": 12,
    "top_topics": ["api", "database", "auth"],
    "sample_facts": [...]
  }]
}
```

### Merging Similar Facts

```
> consolidate(similarity_threshold: 0.7)
"Found 2 similar fact pairs"

> merge_facts(
    fact_a: "uuid-1",
    fact_b: "uuid-2",
    merged_content: "Combined understanding of the auth system"
  )

{
  "merged_id": "new-uuid",
  "archived_ids": ["uuid-1", "uuid-2"],
  "message": "Merged facts. Original facts archived."
}
```

### Cross-Project Queries

```
Claude: What authentication patterns have I seen across all my projects?

> recall(query: "authentication", all_projects: true)

[Returns auth facts from ALL projects, not just the current one]
```

### Memory Maintenance

```
Claude: Time for memory hygiene.

> stats()
{ "total_facts": 150, "total_topics": 45, "stale_facts": 8 }

> decay(threshold_days: 30, decay_factor: 0.9)
"Applied 10% decay to 42 facts not accessed in 30 days"

> prune(days_unused: 90, min_confidence: 0.3, archive: true)
"Archived 5 facts (unused for 90 days, confidence < 0.3)"

> consolidate(similarity_threshold: 0.6)
"Found 3 similar fact pairs that may be redundant"
```

## Configuration

### Database Location

Default: `~/.claude/context-memory/memory.db`

The database is created automatically on first use.

### Auto-Maintenance

On server startup, light maintenance runs automatically. Configure via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `CONTEXT_MEMORY_DECAY_DAYS` | 7 | Days of inactivity before decay |
| `CONTEXT_MEMORY_DECAY_FACTOR` | 0.95 | Decay multiplier (0.95 = 5% decay) |
| `CONTEXT_MEMORY_SKIP_MAINTENANCE` | 0 | Set to "1" to skip startup maintenance |

### Backup

```bash
# Simple backup
cp ~/.claude/context-memory/memory.db ~/backup/memory-$(date +%Y%m%d).db

# SQLite backup (while server may be running)
sqlite3 ~/.claude/context-memory/memory.db ".backup ~/backup/memory.db"
```

## Security

- **Path Traversal Prevention** - Source paths are canonicalized
- **FTS5 Query Sanitization** - Prevents query injection
- **Input Validation** - Length limits on all text fields
- **SQLite Security** - foreign_keys, secure_delete, WAL mode enabled

## Troubleshooting

### "Tool not found" errors

Ensure the plugin is enabled:
```bash
cat ~/.claude/settings.json | grep context-memory
```

### Database locked errors

The server uses WAL mode for concurrent access. If you see lock errors:
```bash
# Check for stale locks
ls -la ~/.claude/context-memory/

# Remove WAL files if server is stopped
rm ~/.claude/context-memory/memory.db-wal
rm ~/.claude/context-memory/memory.db-shm
```

### High memory usage

Run maintenance to clean up:
```
> prune(days_unused: 60, min_confidence: 0.4, archive: false)
> decay(threshold_days: 14, decay_factor: 0.8)
```

## Development

```bash
# Run tests
cargo test

# Build debug
cargo build

# Build release
cargo build --release

# Run with logging
RUST_LOG=debug ./target/debug/context-memory
```

## License

MIT
