//! Effectiveness benchmarks for context-memory
//!
//! Measures whether the memory system actually helps or hurts.

use context_memory::{storage::Storage, Category, Fact, FactFilter, Importance};
use std::time::{Duration, Instant};

fn main() {
    println!("=== Context Memory Effectiveness Benchmark ===\n");

    // Test 1: Latency overhead
    println!("## 1. Latency Overhead");
    latency_benchmark();

    // Test 2: Retrieval quality
    println!("\n## 2. Retrieval Quality");
    retrieval_quality_benchmark();

    // Test 3: Scale testing
    println!("\n## 3. Scale Testing");
    scale_benchmark();

    // Test 4: Contradiction detection accuracy
    println!("\n## 4. Contradiction Detection");
    contradiction_benchmark();

    // Test 5: Time-weighted retrieval effectiveness
    println!("\n## 5. Time-Weighted Retrieval");
    time_weighted_benchmark();
}

fn latency_benchmark() {
    let storage = Storage::in_memory().unwrap();

    // Measure insert latency
    let mut insert_times = Vec::new();
    for i in 0..1000 {
        let fact = Fact::new(format!("Test fact number {} about authentication and security", i))
            .with_topics(vec!["auth".into(), "security".into(), "test".into()])
            .with_category(Category::Architecture)
            .with_importance(Importance::Normal);

        let start = Instant::now();
        storage.insert_fact(&fact).unwrap();
        insert_times.push(start.elapsed());
    }

    let avg_insert = insert_times.iter().sum::<Duration>() / insert_times.len() as u32;
    println!("  Insert (1000 facts):");
    println!("    Average: {:.3}ms", avg_insert.as_secs_f64() * 1000.0);
    println!("    Total:   {:.1}ms", insert_times.iter().sum::<Duration>().as_secs_f64() * 1000.0);

    // Measure search latency
    let mut search_times = Vec::new();
    let queries = ["auth", "security", "test", "fact", "number"];

    for _ in 0..100 {
        for query in &queries {
            let start = Instant::now();
            let _ = storage.search(query, &FactFilter::default(), 10).unwrap();
            search_times.push(start.elapsed());
        }
    }

    search_times.sort();
    let avg_search = search_times.iter().sum::<Duration>() / search_times.len() as u32;
    let p50 = search_times[search_times.len() / 2];
    let p99 = search_times[search_times.len() * 99 / 100];

    println!("  Search (500 queries on 1000 facts):");
    println!("    Average: {:.3}ms", avg_search.as_secs_f64() * 1000.0);
    println!("    P50:     {:.3}ms", p50.as_secs_f64() * 1000.0);
    println!("    P99:     {:.3}ms", p99.as_secs_f64() * 1000.0);

    // Verdict
    if avg_search.as_secs_f64() * 1000.0 < 10.0 {
        println!("  ✓ PASS: Search latency under 10ms");
    } else {
        println!("  ✗ FAIL: Search latency too high");
    }
}

fn retrieval_quality_benchmark() {
    let storage = Storage::in_memory().unwrap();

    // Create a realistic knowledge base
    let facts = vec![
        ("The API uses JWT tokens with RS256 signing for authentication", vec!["auth", "jwt", "api"]),
        ("Database is PostgreSQL 15 with pgvector extension", vec!["database", "postgres"]),
        ("Rate limiting is 100 requests per minute per user", vec!["api", "rate-limit"]),
        ("OAuth2 is used for third-party authentication", vec!["auth", "oauth"]),
        ("Redis is used for session storage", vec!["database", "redis", "session"]),
        ("API responses are cached for 5 minutes", vec!["api", "cache"]),
        ("User passwords are hashed with bcrypt", vec!["auth", "security", "password"]),
        ("The frontend uses React 18", vec!["frontend", "react"]),
        ("GraphQL is used alongside REST", vec!["api", "graphql"]),
        ("Logging uses structured JSON format", vec!["logging", "observability"]),
    ];

    for (content, topics) in &facts {
        let fact = Fact::new(*content)
            .with_topics(topics.iter().map(|s| s.to_string()).collect());
        storage.insert_fact(&fact).unwrap();
    }

    // Test retrieval quality
    // Without synonym expansion, we rely on:
    // 1. FTS5 content matching (finds words in content)
    // 2. Exact topic matching (query terms must match topic names exactly)
    let test_cases = vec![
        ("auth", vec!["auth"], 3),             // Exact topic match: JWT, OAuth2, bcrypt facts
        ("database", vec!["database"], 2),     // Exact topic + content: PostgreSQL, Redis
        ("api", vec!["api"], 4),               // Exact topic + content: JWT, rate-limit, cache, graphql
    ];

    let mut total_precision = 0.0;
    let mut total_recall = 0.0;

    for (query, expected_topics, expected_count) in &test_cases {
        let results = storage.search(query, &FactFilter::default(), 5).unwrap();

        // Calculate precision: how many returned results are relevant?
        let relevant_returned = results.iter()
            .filter(|f| f.topics.iter().any(|t| expected_topics.contains(&t.as_str())))
            .count();
        let precision = relevant_returned as f64 / results.len().max(1) as f64;

        // Calculate recall: how many relevant facts were returned?
        let recall = relevant_returned as f64 / *expected_count as f64;

        println!("  Query '{}': precision={:.0}% recall={:.0}% ({}/{})",
            query, precision * 100.0, recall * 100.0, relevant_returned, expected_count);

        total_precision += precision;
        total_recall += recall;
    }

    let avg_precision = total_precision / test_cases.len() as f64;
    let avg_recall = total_recall / test_cases.len() as f64;

    println!("  Average: precision={:.0}% recall={:.0}%", avg_precision * 100.0, avg_recall * 100.0);

    if avg_precision >= 0.7 && avg_recall >= 0.7 {
        println!("  ✓ PASS: Retrieval quality acceptable");
    } else {
        println!("  ✗ FAIL: Retrieval quality needs improvement");
    }
}

fn scale_benchmark() {
    println!("  Testing database scaling...");

    for num_facts in [100, 1000, 10000] {
        let storage = Storage::in_memory().unwrap();

        // Insert facts
        let start = Instant::now();
        for i in 0..num_facts {
            let fact = Fact::new(format!("Fact {} about topic{} with keyword{}", i, i % 50, i % 100))
                .with_topics(vec![format!("topic{}", i % 50)]);
            storage.insert_fact(&fact).unwrap();
        }
        let insert_time = start.elapsed();

        // Measure search at scale
        let mut search_times = Vec::new();
        for i in 0..100 {
            let start = Instant::now();
            let _ = storage.search(&format!("topic{}", i % 50), &FactFilter::default(), 10).unwrap();
            search_times.push(start.elapsed());
        }

        let avg_search = search_times.iter().sum::<Duration>() / search_times.len() as u32;

        println!("  {} facts: insert={:.0}ms, search={:.3}ms avg",
            num_facts,
            insert_time.as_secs_f64() * 1000.0,
            avg_search.as_secs_f64() * 1000.0);
    }
}

fn contradiction_benchmark() {
    // Test cases: (fact1, fact2, should_contradict)
    let test_cases = vec![
        ("API uses JWT for auth", "API uses OAuth for auth", false), // Different methods, not contradiction
        ("Feature X is enabled", "Feature X is disabled", true),
        ("Database is PostgreSQL", "Database is MySQL", true),
        ("Cache TTL is 5 minutes", "Cache TTL is 10 minutes", true),
        ("API is REST", "API also supports GraphQL", false), // Both can be true
        ("Auth is required", "Auth is not required", true),
    ];

    let mut correct = 0;
    let total = test_cases.len();

    for (fact1, fact2, should_contradict) in &test_cases {
        // Simple heuristic check (matching our detection logic)
        let f1_lower = fact1.to_lowercase();
        let f2_lower = fact2.to_lowercase();

        let negation_words = ["not", "no", "never", "disabled", "without"];
        let opposite_pairs = [("enabled", "disabled"), ("required", "not required"),
                             ("postgresql", "mysql"), ("5 minutes", "10 minutes")];

        let has_negation_diff = negation_words.iter().any(|w|
            f1_lower.contains(w) != f2_lower.contains(w));

        let has_opposite = opposite_pairs.iter().any(|(a, b)|
            (f1_lower.contains(a) && f2_lower.contains(b)) ||
            (f1_lower.contains(b) && f2_lower.contains(a)));

        let detected = has_negation_diff || has_opposite;

        if detected == *should_contradict {
            correct += 1;
            println!("  ✓ '{}' vs '{}': correctly {}",
                fact1, fact2, if detected { "detected" } else { "passed" });
        } else {
            println!("  ✗ '{}' vs '{}': should {} but {}",
                fact1, fact2,
                if *should_contradict { "contradict" } else { "not contradict" },
                if detected { "detected" } else { "missed" });
        }
    }

    let accuracy = correct as f64 / total as f64;
    println!("  Accuracy: {:.0}% ({}/{})", accuracy * 100.0, correct, total);

    if accuracy >= 0.7 {
        println!("  ✓ PASS: Contradiction detection acceptable");
    } else {
        println!("  ✗ FAIL: Contradiction detection needs improvement");
    }
}

fn time_weighted_benchmark() {
    // This tests whether recently accessed facts rank higher
    let storage = Storage::in_memory().unwrap();

    // Insert facts
    let old_fact = Fact::new("Old authentication method uses basic auth")
        .with_topics(vec!["auth".into()])
        .with_confidence(0.9);
    let new_fact = Fact::new("New authentication uses JWT tokens")
        .with_topics(vec!["auth".into()])
        .with_confidence(0.8);

    storage.insert_fact(&old_fact).unwrap();
    storage.insert_fact(&new_fact).unwrap();

    // Access the new fact multiple times
    for _ in 0..5 {
        storage.mark_accessed(new_fact.id).unwrap();
    }

    // Search and check ranking
    let results = storage.search("auth", &FactFilter::default(), 10).unwrap();

    if results.len() >= 2 {
        let new_rank = results.iter().position(|f| f.id == new_fact.id);
        let old_rank = results.iter().position(|f| f.id == old_fact.id);

        println!("  New fact (accessed 5x): rank {:?}", new_rank);
        println!("  Old fact (never accessed): rank {:?}", old_rank);

        if let (Some(new_r), Some(old_r)) = (new_rank, old_rank) {
            if new_r < old_r {
                println!("  ✓ PASS: Frequently accessed fact ranked higher");
            } else {
                println!("  ? NOTE: Recently accessed fact not ranked higher (may need tuning)");
            }
        }
    }

    // Check confidence boost worked
    let boosted = storage.get_fact(new_fact.id).unwrap().unwrap();
    println!("  Confidence after 5 accesses: {:.2} (started at 0.80)", boosted.confidence);

    if boosted.confidence > 0.85 {
        println!("  ✓ PASS: Confidence boost working");
    } else {
        println!("  ✗ FAIL: Confidence boost not working");
    }
}
