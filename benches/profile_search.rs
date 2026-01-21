//! Profile search to find the bottleneck

use context_memory::{storage::Storage, Fact, FactFilter};
use std::time::Instant;

fn main() {
    println!("=== Search Profiling ===\n");

    // Test 1: get_project_root() overhead
    println!("1. get_project_root():");
    let mut times = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        let _ = context_memory::get_project_root();
        times.push(start.elapsed());
    }
    let avg = times.iter().sum::<std::time::Duration>() / times.len() as u32;
    println!("  Average: {:.3}ms", avg.as_secs_f64() * 1000.0);

    // Test 2: Query tokenization overhead
    println!("\n2. Query tokenization:");
    let mut times = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        let _ = context_memory::tokenize_query("authentication security test");
        times.push(start.elapsed());
    }
    let avg = times.iter().sum::<std::time::Duration>() / times.len() as u32;
    println!("  Average: {:.3}ms", avg.as_secs_f64() * 1000.0);

    // Create storage with facts
    let storage = Storage::in_memory().unwrap();
    for i in 0..100 {
        let fact = Fact::new(format!("Test fact {} about authentication security", i))
            .with_topics(vec!["auth".into(), "security".into()]);
        storage.insert_fact(&fact).unwrap();
    }

    // Test 3: Full search through Storage
    println!("\n3. Full Storage.search():");
    let mut times = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        let _ = storage.search("auth", &FactFilter::default(), 10).unwrap();
        times.push(start.elapsed());
    }
    let avg = times.iter().sum::<std::time::Duration>() / times.len() as u32;
    println!("  Average: {:.3}ms", avg.as_secs_f64() * 1000.0);

    // Test 4: Search with all_projects=true (skip project root lookup)
    println!("\n4. Storage.search() with all_projects=true:");
    let mut times = Vec::new();
    let filter = FactFilter { all_projects: true, ..Default::default() };
    for _ in 0..100 {
        let start = Instant::now();
        let _ = storage.search("auth", &filter, 10).unwrap();
        times.push(start.elapsed());
    }
    let avg = times.iter().sum::<std::time::Duration>() / times.len() as u32;
    println!("  Average: {:.3}ms", avg.as_secs_f64() * 1000.0);

    // Test 5: Search with empty query (no FTS/synonyms)
    println!("\n5. Storage.search() with empty query:");
    let mut times = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        let _ = storage.search("", &filter, 10).unwrap();
        times.push(start.elapsed());
    }
    let avg = times.iter().sum::<std::time::Duration>() / times.len() as u32;
    println!("  Average: {:.3}ms", avg.as_secs_f64() * 1000.0);

    // Test 6: Raw FTS5 query (no abstraction)
    println!("\n6. Raw FTS5 query:");
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(r#"
        CREATE TABLE test(content TEXT);
        CREATE VIRTUAL TABLE test_fts USING fts5(content, content='test', content_rowid='rowid');
        CREATE TRIGGER test_ai AFTER INSERT ON test BEGIN
            INSERT INTO test_fts(rowid, content) VALUES (NEW.rowid, NEW.content);
        END;
    "#).unwrap();
    for i in 0..100 {
        conn.execute("INSERT INTO test(content) VALUES (?)", [format!("Test fact {} about auth", i)]).unwrap();
    }

    let mut times = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        let mut stmt = conn.prepare("SELECT rowid FROM test_fts WHERE test_fts MATCH ?").unwrap();
        let _results: Vec<i64> = stmt.query_map(["\"auth\""], |row| row.get(0)).unwrap()
            .collect::<Result<Vec<_>, _>>().unwrap();
        times.push(start.elapsed());
    }
    let avg = times.iter().sum::<std::time::Duration>() / times.len() as u32;
    println!("  Average: {:.3}ms", avg.as_secs_f64() * 1000.0);
}
