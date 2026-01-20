# Context Memory

Context-rot-resistant memory MCP server for Claude Code.

## Features

- **Persistent Memory** - SQLite storage with FTS5 full-text search
- **Source Provenance** - Track where facts came from (file:line)
- **Staleness Detection** - Detect when source code changes invalidate facts
- **Relationships** - Link facts together (depends_on, contradicts, elaborates, etc.)
- **Contradiction Awareness** - Find conflicting information
- **Rich Metadata** - Categories, importance, certainty, scope, topics

## Installation

```bash
cargo build --release
```

## Usage

Add to your Claude Code MCP configuration:

```json
{
  "mcpServers": {
    "context-memory": {
      "command": "/path/to/context-memory",
      "args": ["--db", "~/.context-memory.db"]
    }
  }
}
```

## MCP Tools

### Core Memory

| Tool | Description |
|------|-------------|
| `remember` | Store a fact with optional source, topics, and metadata |
| `recall` | Search facts by keyword with filters |
| `forget` | Remove a fact |
| `forget_observation` | Remove specific evidence from a fact |

### Verification

| Tool | Description |
|------|-------------|
| `verify` | Check if a fact's source file has changed |
| `get_stale` | List facts that need verification |
| `refresh_all` | Batch verify all facts with file sources |

### Relationships

| Tool | Description |
|------|-------------|
| `link` | Connect two facts with a relationship |
| `unlink` | Remove a relationship |
| `get_related` | Find facts connected to a given fact |
| `find_contradictions` | Detect contradicting facts |

### Exploration

| Tool | Description |
|------|-------------|
| `list_topics` | List all topics with counts |
| `summarize` | Get facts on a specific topic |
| `stats` | Get memory statistics |

## Example

```
Claude: I'll remember that the auth module uses JWT tokens.

> remember(
    content: "Auth module uses JWT tokens for session management",
    source: "src/auth/mod.rs:42",
    topics: ["auth", "jwt", "sessions"],
    category: "architecture",
    importance: "high"
  )

Claude: Later, let me check what I know about auth...

> recall(query: "authentication", topics: ["auth"])

[Returns the JWT fact with staleness status]
```

## Security

- Path traversal prevention with canonicalization
- FTS5 query sanitization
- Input validation with length limits
- SQLite security pragmas (foreign_keys, secure_delete, WAL mode)

## License

MIT
