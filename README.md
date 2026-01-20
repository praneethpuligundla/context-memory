# RLM - Recursive Language Models in Rust 🦀

Pure Rust implementation of [Recursive Language Models](https://arxiv.org/abs/2512.24601) for processing arbitrarily long contexts with Claude.

## Features

- **Pure Rust** - No Python dependency
- **Fast** - SIMD regex, Rayon parallelism
- **CLI** - Process files directly from terminal
- **Library** - Use in your Rust projects

## Build

```bash
# Build release
cargo build --release

# Run
./target/release/rlm --help
```

## CLI Usage

```bash
# Set API key
export ANTHROPIC_API_KEY="your-key"

# Process a file
rlm --file huge_log.txt --task "Find all errors and summarize"

# Pipe input
cat codebase.txt | rlm --task "Find security vulnerabilities"

# With options
rlm --file data.txt \
    --task "Extract all dates" \
    --model claude-sonnet-4-20250514 \
    --sub-model claude-haiku-4-5-20251001 \
    --max-iterations 15 \
    --verbose

# JSON output
rlm --file doc.txt --task "Summarize" --json
```

## Library Usage

```rust
use rlm::{RLM, RLMConfig, ContextManager};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Quick usage
    let answer = rlm::run_rlm(
        &std::fs::read_to_string("huge_file.txt")?,
        "Find the secret number"
    ).await?;
    println!("{}", answer);

    // With configuration
    let config = RLMConfig {
        root_model: "claude-sonnet-4-20250514".into(),
        sub_model: "claude-haiku-4-5-20251001".into(),
        max_iterations: 15,
        verbose: true,
        ..Default::default()
    };

    let rlm = RLM::new(config);
    let result = rlm.completion(&context, "Find all TODOs").await?;
    
    println!("Answer: {}", result.response);
    println!("Iterations: {}", result.iterations);
    println!("Sub-calls: {}", result.sub_calls);

    Ok(())
}
```

## Context Manager

Fast operations on large text:

```rust
use rlm::ContextManager;

let mut ctx = ContextManager::new(huge_text);

// O(1) slicing
let head = ctx.head(1000);
let tail = ctx.tail(1000);
let slice = ctx.slice(50000, 60000);

// Fast chunking
let chunks = ctx.chunk_by_size(10000, 500);  // With overlap
let chunks = ctx.chunk_by_delimiter("\n\n");  // By paragraph

// SIMD regex
let matches = ctx.find_pattern(r"\d{4}-\d{2}-\d{2}")?;
let results = ctx.find_with_context(r"ERROR", 500)?;

// Parallel search (Rayon)
let indices = ctx.parallel_search(r"warning|error")?;

// Build index for O(1) lookups
ctx.build_index(&["error", "warning", "critical"]);
let positions = ctx.lookup("error");

// Stats
let stats = ctx.stats();  // chars, lines, words, chunks
```

## Performance

| Operation | Time (10MB text) |
|-----------|-----------------|
| Load | 12ms |
| chunk_by_size | 7ms |
| find_pattern | 11ms |
| parallel_search | 45ms (8 cores) |

## How It Works

1. Context stored externally (not in LLM prompt)
2. Claude writes code to explore/slice context
3. Code execution returns results
4. Claude calls `llm_query()` on relevant chunks
5. Results aggregated into final answer

```
┌──────────────────────────────────────────┐
│           Root LLM (Claude)              │
│  - Writes exploration code               │
│  - Calls llm_query() on chunks           │
│  - Aggregates results                    │
└────────────────┬─────────────────────────┘
                 │
    ┌────────────┴────────────┐
    ▼                         ▼
┌─────────┐              ┌─────────┐
│ Chunk 1 │              │ Chunk 2 │
│ Sub-LLM │              │ Sub-LLM │
└─────────┘              └─────────┘
```

## License

MIT
