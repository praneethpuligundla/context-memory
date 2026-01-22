# Context Memory

Context-rot-resistant memory MCP server for Claude Code. Gives Claude persistent memory that survives across sessions, with automatic staleness detection when source code changes.

## Features

### Core Memory
- **Persistent Storage** - Facts survive across sessions in SQLite with FTS5 full-text search
- **Source Tracking** - Link facts to source files (`file:line`); detect when code changes invalidate them
- **Smart Retrieval** - Time-weighted search ranks recent, high-confidence facts higher
- **Project Isolation** - Facts auto-scoped to git repository; cross-project queries supported

### Intelligence
- **Contradiction Detection** - Automatic warnings when new facts conflict with existing ones
- **Session Tracking** - Know what was learned in the current conversation
- **Fact Versioning** - Track history of changes to facts over time
- **Topic Extraction** - Auto-extract topics from content using hashtags and keywords

### Memory Hygiene
- **Confidence Decay** - Unused facts lose confidence over time
- **Confidence Boost** - Frequently accessed facts gain confidence (+2% per access)
- **Auto-Maintenance** - Light decay runs on server startup
- **Pruning** - Remove or archive old, low-confidence facts
- **Consolidation** - Find and merge similar/duplicate facts

### Relationships
- **Fact Linking** - Connect facts with typed relations (depends_on, contradicts, elaborates, etc.)
- **Supersedes** - Track when facts replace older versions
- **Category Summaries** - View facts grouped by category

## Architecture

### Daemon/Client Model

Context Memory uses a daemon architecture for concurrent session support. Multiple Claude Code sessions can safely share the same memory database.

```
┌──────────────────┐   ┌──────────────────┐   ┌──────────────────┐
│  Claude Code #1  │   │  Claude Code #2  │   │  Claude Code #3  │
│                  │   │                  │   │                  │
│  MCP (stdio)     │   │  MCP (stdio)     │   │  MCP (stdio)     │
└────────┬─────────┘   └────────┬─────────┘   └────────┬─────────┘
         │                      │                      │
         ▼                      ▼                      ▼
┌──────────────────┐   ┌──────────────────┐   ┌──────────────────┐
│  Client Process  │   │  Client Process  │   │  Client Process  │
│  (stdio bridge)  │   │  (stdio bridge)  │   │  (stdio bridge)  │
└────────┬─────────┘   └────────┬─────────┘   └────────┬─────────┘
         │                      │                      │
         └──────────────────────┼──────────────────────┘
                                │
                    Unix Socket │ (~/.claude/context-memory/daemon.sock)
                                ▼
         ┌──────────────────────────────────────────────────────┐
         │                    Daemon Process                     │
         │                                                       │
         │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐     │
         │  │ MCP Server  │ │ MCP Server  │ │ MCP Server  │     │
         │  │ (session 1) │ │ (session 2) │ │ (session 3) │     │
         │  └──────┬──────┘ └──────┬──────┘ └──────┬──────┘     │
         │         └───────────────┼───────────────┘            │
         │                         ▼                            │
         │              ┌─────────────────────┐                 │
         │              │   Shared Storage    │                 │
         │              │   (Connection Pool) │                 │
         │              └──────────┬──────────┘                 │
         └─────────────────────────┼────────────────────────────┘
                                   │
                                   ▼
                     ~/.claude/context-memory/memory.db
```

**How it works:**
1. **Client mode** (default): Spawned by Claude Code, bridges stdio ↔ daemon socket
2. **Daemon mode** (`--daemon`): Long-running process accepting concurrent connections
3. **Auto-start**: Client auto-starts daemon if not running

### Internal Architecture

```
┌────────────────────────────────────────────────────────────────────────────┐
│                         Context Memory Server                               │
│                                                                            │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                          Tool Router (rmcp)                          │  │
│  │                                                                      │  │
│  │  remember  recall  forget  verify  link  decay  prune  merge  ...   │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                                    │                                       │
│                                    ▼                                       │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                          Tool Handler                                │  │
│  │                                                                      │  │
│  │  • Input validation          • Contradiction detection               │  │
│  │  • Topic extraction          • Session ID injection                  │  │
│  │  • Source hash computation   • Confidence management                 │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                                    │                                       │
│                                    ▼                                       │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                         Storage Layer                                │  │
│  │                                                                      │  │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌────────────┐  │  │
│  │  │   facts     │  │  relations  │  │fact_history │  │ facts_fts  │  │  │
│  │  │             │  │             │  │             │  │  (FTS5)    │  │  │
│  │  │ • content   │  │ • from_id   │  │ • version   │  │            │  │  │
│  │  │ • source    │  │ • to_id     │  │ • content   │  │ Full-text  │  │  │
│  │  │ • confidence│  │ • type      │  │ • changed_at│  │ search     │  │  │
│  │  │ • topics    │  │ • metadata  │  │             │  │ index      │  │  │
│  │  │ • session_id│  │             │  │             │  │            │  │  │
│  │  └─────────────┘  └─────────────┘  └─────────────┘  └────────────┘  │  │
│  │                                                                      │  │
│  │                        SQLite + WAL Mode                             │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────────┘
```

### Data Flow

```
┌─────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  remember   │────▶│ Validate Input   │────▶│ Extract Topics  │
│  (new fact) │     │ Check Length     │     │ from Content    │
└─────────────┘     └──────────────────┘     └─────────────────┘
                                                      │
                    ┌──────────────────┐              ▼
                    │ Check for        │◀─────┌─────────────────┐
                    │ Contradictions   │      │ Compute Source  │
                    │ (topic overlap)  │      │ Hash (SHA256)   │
                    └──────────────────┘      └─────────────────┘
                            │
                            ▼
┌─────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  Return ID  │◀────│ Store in SQLite  │◀────│ Add Session ID  │
│ + Warnings  │     │ + FTS5 Index     │     │ + Project Path  │
└─────────────┘     └──────────────────┘     └─────────────────┘
```

```
┌─────────────┐     ┌──────────────────┐     ┌─────────────────┐
│   recall    │────▶│ Sanitize Query   │────▶│ Build SQL with  │
│  (search)   │     │ (FTS5 injection) │     │ Filters         │
└─────────────┘     └──────────────────┘     └─────────────────┘
                                                      │
                    ┌──────────────────┐              ▼
                    │ Boost Confidence │◀─────┌─────────────────┐
                    │ (+2% per access) │      │ Time-Weighted   │
                    └──────────────────┘      │ Scoring         │
                            │                 └─────────────────┘
                            ▼
                    ┌──────────────────┐
                    │ Return Ranked    │
                    │ Facts            │
                    └──────────────────┘
```

### Data Model

```
Fact
├── id (UUID)
├── content (the fact itself)
├── project_path (git root - for project isolation)
├── session_id (conversation tracking)
├── source (file:line reference)
├── source_content_hash (SHA256 for staleness)
├── confidence (0.0-1.0)
├── importance (critical/high/normal/low)
├── certainty (definite/likely/uncertain/speculative)
├── category (architecture/decision/pattern/bug/todo/...)
├── scope (global/project/branch/task)
├── topics[] (tags)
├── evidence[] (supporting observations)
├── access_count
├── last_accessed
├── stale (boolean)
└── archived (boolean)

Fact History
├── fact_id
├── version
├── content (snapshot)
├── confidence (snapshot)
├── changed_at
└── change_reason

Relations
├── from_id → to_id
├── type (depends_on/contradicts/elaborates/related_to/part_of/supersedes)
└── metadata
```

## Installation

### Prerequisites

- Rust 1.70+ (for building)
- Claude Code CLI

### Quick Start

```bash
# Clone and build
git clone https://github.com/praneethpuligundla/context-memory
cd context-memory
cargo build --release

# Add to Claude Code MCP config (~/.claude/.mcp.json)
{
  "mcpServers": {
    "context-memory": {
      "command": "/absolute/path/to/context-memory/target/release/context-memory",
      "args": []
    }
  }
}

# Restart Claude Code
```

### Verify Installation

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

## How It Works

### Time-Weighted Retrieval

```
score = confidence × importance_weight × time_decay

where:
  importance_weight = { critical: 4, high: 2, normal: 1, low: 0.5 }
  time_decay = 1 / (1 + days_since_access / 30)
```

### Staleness Detection

When a fact references a source file (e.g., `src/auth.rs:42`):
1. Computes SHA256 hash of file content on store
2. On `verify()`, recomputes hash and compares
3. Marks fact as `stale: true` if changed

### Contradiction Detection

When storing new facts:
1. Finds existing facts with >50% topic overlap (Jaccard similarity)
2. Analyzes content for negation words (`not`, `never`, `don't`, etc.)
3. Checks for opposite value pairs (`true/false`, `enabled/disabled`, etc.)
4. Returns warnings but still stores the fact
5. Auto-creates `contradicts` relation for high-confidence conflicts

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CONTEXT_MEMORY_DECAY_DAYS` | 7 | Days of inactivity before decay |
| `CONTEXT_MEMORY_DECAY_FACTOR` | 0.95 | Decay multiplier (0.95 = 5% decay) |
| `CONTEXT_MEMORY_SKIP_MAINTENANCE` | 0 | Set to "1" to skip startup maintenance |

### Files

| Path | Description |
|------|-------------|
| `~/.claude/context-memory/memory.db` | SQLite database |
| `~/.claude/context-memory/daemon.sock` | Unix socket for client↔daemon IPC |
| `~/.claude/context-memory/daemon.pid` | PID file for daemon process |

### Command Line

```bash
context-memory          # Run in client mode (default, used by Claude Code)
context-memory --daemon # Run as daemon (auto-started by client if needed)
```

## Security

- **Path Traversal Prevention** - Source paths are canonicalized
- **FTS5 Query Sanitization** - Prevents query injection
- **Input Validation** - Length limits on all text fields
- **SQLite Security** - foreign_keys, secure_delete, WAL mode enabled

## Development

```bash
cargo test          # Run tests
cargo build         # Build debug
cargo build --release  # Build release
RUST_LOG=debug ./target/debug/context-memory  # Run with logging
```

## License

MIT
