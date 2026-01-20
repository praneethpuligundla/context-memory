//! SQLite storage layer with FTS5 full-text search.

use crate::error::Result;
use crate::types::{
    Category, Certainty, Fact, FactFilter, Importance, Relation, RelationType, Scope, SourceType,
};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use uuid::Uuid;

/// Sanitize a query string for FTS5 to prevent query injection.
///
/// FTS5 has special operators that could cause unexpected behavior:
/// - `*` (prefix search)
/// - `^` (start of column)
/// - `NEAR` operator
/// - `OR`, `AND`, `NOT` operators
/// - Quotes for phrase matching
/// - Parentheses for grouping
///
/// This function escapes double quotes and wraps terms in quotes for literal matching.
fn sanitize_fts5_query(query: &str) -> String {
    if query.is_empty() {
        return String::new();
    }

    // Split into words and quote each term for literal matching
    // This prevents FTS5 operator injection
    query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .map(|term| {
            // Escape any existing double quotes
            let escaped = term.replace('"', "\"\"");
            format!("\"{}\"", escaped)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// SQLite-backed storage for facts and relations.
pub struct Storage {
    conn: Connection,
}

impl Storage {
    /// Create new storage, initializing database at path.
    pub fn new(db_path: impl AsRef<Path>) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)?;
        let storage = Self { conn };
        storage.init_schema()?;
        Ok(storage)
    }

    /// Create in-memory storage for testing.
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let storage = Self { conn };
        storage.init_schema()?;
        Ok(storage)
    }

    fn init_schema(&self) -> Result<()> {
        // Security: Enable foreign key enforcement (disabled by default in SQLite)
        self.conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        // Reliability: Set busy timeout to prevent lock contention errors (5 seconds)
        self.conn.execute_batch("PRAGMA busy_timeout = 5000;")?;

        // Performance: Use WAL mode for better concurrent access
        self.conn.execute_batch("PRAGMA journal_mode = WAL;")?;

        // Security: Ensure secure delete (overwrite deleted data)
        self.conn.execute_batch("PRAGMA secure_delete = ON;")?;

        self.conn.execute_batch(
            r#"
            -- Main facts table
            CREATE TABLE IF NOT EXISTS facts (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,

                -- Source provenance
                source TEXT,
                source_type TEXT NOT NULL DEFAULT 'manual',
                source_content_hash TEXT,
                git_commit TEXT,

                -- Confidence & lifecycle
                confidence REAL NOT NULL DEFAULT 0.8,
                certainty TEXT NOT NULL DEFAULT 'likely',
                created_at TEXT NOT NULL,
                last_verified TEXT NOT NULL,
                stale INTEGER NOT NULL DEFAULT 0,

                -- Categorization
                category TEXT NOT NULL DEFAULT 'context',
                importance TEXT NOT NULL DEFAULT 'normal',
                scope TEXT NOT NULL DEFAULT 'project',

                -- Provenance chain
                derived_from TEXT,
                supersedes TEXT,

                -- Usage tracking
                access_count INTEGER NOT NULL DEFAULT 0,
                last_accessed TEXT
            );

            -- Topics (many-to-many)
            CREATE TABLE IF NOT EXISTS fact_topics (
                fact_id TEXT NOT NULL,
                topic TEXT NOT NULL,
                PRIMARY KEY (fact_id, topic),
                FOREIGN KEY (fact_id) REFERENCES facts(id) ON DELETE CASCADE
            );

            -- Evidence
            CREATE TABLE IF NOT EXISTS fact_evidence (
                fact_id TEXT NOT NULL,
                evidence TEXT NOT NULL,
                FOREIGN KEY (fact_id) REFERENCES facts(id) ON DELETE CASCADE
            );

            -- Relations between facts
            CREATE TABLE IF NOT EXISTS relations (
                from_id TEXT NOT NULL,
                to_id TEXT NOT NULL,
                relation_type TEXT NOT NULL,
                created_at TEXT NOT NULL,
                metadata TEXT,
                PRIMARY KEY (from_id, to_id, relation_type),
                FOREIGN KEY (from_id) REFERENCES facts(id) ON DELETE CASCADE,
                FOREIGN KEY (to_id) REFERENCES facts(id) ON DELETE CASCADE
            );

            -- Checkpoints for session management
            CREATE TABLE IF NOT EXISTS checkpoints (
                name TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                fact_count INTEGER NOT NULL
            );

            -- Task contexts
            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                started_at TEXT NOT NULL,
                ended_at TEXT
            );

            CREATE TABLE IF NOT EXISTS task_facts (
                task_id TEXT NOT NULL,
                fact_id TEXT NOT NULL,
                PRIMARY KEY (task_id, fact_id),
                FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE,
                FOREIGN KEY (fact_id) REFERENCES facts(id) ON DELETE CASCADE
            );

            -- Full-text search index
            CREATE VIRTUAL TABLE IF NOT EXISTS facts_fts USING fts5(
                content,
                content='facts',
                content_rowid='rowid'
            );

            -- Triggers to keep FTS in sync
            CREATE TRIGGER IF NOT EXISTS facts_ai AFTER INSERT ON facts BEGIN
                INSERT INTO facts_fts(rowid, content) VALUES (NEW.rowid, NEW.content);
            END;

            CREATE TRIGGER IF NOT EXISTS facts_ad AFTER DELETE ON facts BEGIN
                INSERT INTO facts_fts(facts_fts, rowid, content) VALUES('delete', OLD.rowid, OLD.content);
            END;

            CREATE TRIGGER IF NOT EXISTS facts_au AFTER UPDATE ON facts BEGIN
                INSERT INTO facts_fts(facts_fts, rowid, content) VALUES('delete', OLD.rowid, OLD.content);
                INSERT INTO facts_fts(rowid, content) VALUES (NEW.rowid, NEW.content);
            END;

            -- Indexes
            CREATE INDEX IF NOT EXISTS idx_facts_category ON facts(category);
            CREATE INDEX IF NOT EXISTS idx_facts_importance ON facts(importance);
            CREATE INDEX IF NOT EXISTS idx_facts_scope ON facts(scope);
            CREATE INDEX IF NOT EXISTS idx_facts_stale ON facts(stale);
            CREATE INDEX IF NOT EXISTS idx_facts_created ON facts(created_at);
            CREATE INDEX IF NOT EXISTS idx_fact_topics_topic ON fact_topics(topic);
            "#,
        )?;
        Ok(())
    }

    // ========================================================================
    // Fact CRUD
    // ========================================================================

    /// Store a new fact.
    pub fn insert_fact(&self, fact: &Fact) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        tx.execute(
            r#"
            INSERT INTO facts (
                id, content, source, source_type, source_content_hash, git_commit,
                confidence, certainty, created_at, last_verified, stale,
                category, importance, scope, derived_from, supersedes,
                access_count, last_accessed
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18
            )
            "#,
            params![
                fact.id.to_string(),
                fact.content,
                fact.source,
                format!("{:?}", fact.source_type).to_lowercase(),
                fact.source_content_hash,
                fact.git_commit,
                fact.confidence,
                format!("{:?}", fact.certainty).to_lowercase(),
                fact.created_at.to_rfc3339(),
                fact.last_verified.to_rfc3339(),
                fact.stale as i32,
                format!("{:?}", fact.category).to_lowercase(),
                format!("{:?}", fact.importance).to_lowercase(),
                format!("{:?}", fact.scope).to_lowercase(),
                fact.derived_from.map(|u| u.to_string()),
                fact.supersedes.map(|u| u.to_string()),
                fact.access_count,
                fact.last_accessed.map(|d| d.to_rfc3339()),
            ],
        )?;

        // Insert topics
        for topic in &fact.topics {
            tx.execute(
                "INSERT OR IGNORE INTO fact_topics (fact_id, topic) VALUES (?1, ?2)",
                params![fact.id.to_string(), topic],
            )?;
        }

        // Insert evidence
        for evidence in &fact.evidence {
            tx.execute(
                "INSERT INTO fact_evidence (fact_id, evidence) VALUES (?1, ?2)",
                params![fact.id.to_string(), evidence],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Get a fact by ID.
    pub fn get_fact(&self, id: Uuid) -> Result<Option<Fact>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, content, source, source_type, source_content_hash, git_commit,
                   confidence, certainty, created_at, last_verified, stale,
                   category, importance, scope, derived_from, supersedes,
                   access_count, last_accessed
            FROM facts WHERE id = ?1
            "#,
        )?;

        let fact = stmt
            .query_row(params![id.to_string()], |row| self.row_to_fact(row))
            .optional()?;

        match fact {
            Some(mut f) => {
                f.topics = self.get_topics(id)?;
                f.evidence = self.get_evidence(id)?;
                Ok(Some(f))
            }
            None => Ok(None),
        }
    }

    /// Update a fact.
    pub fn update_fact(&self, fact: &Fact) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        tx.execute(
            r#"
            UPDATE facts SET
                content = ?2, source = ?3, source_type = ?4, source_content_hash = ?5,
                git_commit = ?6, confidence = ?7, certainty = ?8, last_verified = ?9,
                stale = ?10, category = ?11, importance = ?12, scope = ?13,
                derived_from = ?14, supersedes = ?15, access_count = ?16, last_accessed = ?17
            WHERE id = ?1
            "#,
            params![
                fact.id.to_string(),
                fact.content,
                fact.source,
                format!("{:?}", fact.source_type).to_lowercase(),
                fact.source_content_hash,
                fact.git_commit,
                fact.confidence,
                format!("{:?}", fact.certainty).to_lowercase(),
                fact.last_verified.to_rfc3339(),
                fact.stale as i32,
                format!("{:?}", fact.category).to_lowercase(),
                format!("{:?}", fact.importance).to_lowercase(),
                format!("{:?}", fact.scope).to_lowercase(),
                fact.derived_from.map(|u| u.to_string()),
                fact.supersedes.map(|u| u.to_string()),
                fact.access_count,
                fact.last_accessed.map(|d| d.to_rfc3339()),
            ],
        )?;

        // Update topics
        tx.execute(
            "DELETE FROM fact_topics WHERE fact_id = ?1",
            params![fact.id.to_string()],
        )?;
        for topic in &fact.topics {
            tx.execute(
                "INSERT INTO fact_topics (fact_id, topic) VALUES (?1, ?2)",
                params![fact.id.to_string(), topic],
            )?;
        }

        // Update evidence
        tx.execute(
            "DELETE FROM fact_evidence WHERE fact_id = ?1",
            params![fact.id.to_string()],
        )?;
        for evidence in &fact.evidence {
            tx.execute(
                "INSERT INTO fact_evidence (fact_id, evidence) VALUES (?1, ?2)",
                params![fact.id.to_string(), evidence],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Delete a fact.
    pub fn delete_fact(&self, id: Uuid) -> Result<bool> {
        let rows = self.conn.execute(
            "DELETE FROM facts WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(rows > 0)
    }

    // ========================================================================
    // Search & Query
    // ========================================================================

    /// Full-text search with optional filters.
    pub fn search(&self, query: &str, filter: &FactFilter, limit: usize) -> Result<Vec<Fact>> {
        let mut sql = String::from(
            r#"
            SELECT f.id, f.content, f.source, f.source_type, f.source_content_hash,
                   f.git_commit, f.confidence, f.certainty, f.created_at, f.last_verified,
                   f.stale, f.category, f.importance, f.scope, f.derived_from, f.supersedes,
                   f.access_count, f.last_accessed
            FROM facts f
            "#,
        );

        let mut conditions = Vec::new();
        // Sanitize query to prevent FTS5 injection
        let sanitized_query = sanitize_fts5_query(query);
        let use_fts = !sanitized_query.is_empty();

        if use_fts {
            sql.push_str("JOIN facts_fts fts ON f.rowid = fts.rowid ");
            conditions.push("facts_fts MATCH ?");
        }

        if filter.category.is_some() {
            conditions.push("f.category = ?");
        }
        if filter.importance.is_some() {
            conditions.push("f.importance = ?");
        }
        if filter.scope.is_some() {
            conditions.push("f.scope = ?");
        }
        if filter.source_type.is_some() {
            conditions.push("f.source_type = ?");
        }
        if filter.stale.is_some() {
            conditions.push("f.stale = ?");
        }
        if filter.min_confidence.is_some() {
            conditions.push("f.confidence >= ?");
        }
        if filter.certainty.is_some() {
            conditions.push("f.certainty = ?");
        }
        if filter.topics.is_some() {
            sql.push_str("JOIN fact_topics ft ON f.id = ft.fact_id ");
            conditions.push("ft.topic IN (SELECT value FROM json_each(?))");
        }

        if !conditions.is_empty() {
            sql.push_str("WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }

        sql.push_str(" ORDER BY f.importance DESC, f.confidence DESC LIMIT ?");

        let mut stmt = self.conn.prepare(&sql)?;

        let mut bind_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if use_fts {
            bind_params.push(Box::new(sanitized_query));
        }
        if let Some(cat) = &filter.category {
            bind_params.push(Box::new(format!("{:?}", cat).to_lowercase()));
        }
        if let Some(imp) = &filter.importance {
            bind_params.push(Box::new(format!("{:?}", imp).to_lowercase()));
        }
        if let Some(scope) = &filter.scope {
            bind_params.push(Box::new(format!("{:?}", scope).to_lowercase()));
        }
        if let Some(st) = &filter.source_type {
            bind_params.push(Box::new(format!("{:?}", st).to_lowercase()));
        }
        if let Some(stale) = &filter.stale {
            bind_params.push(Box::new(*stale as i32));
        }
        if let Some(conf) = &filter.min_confidence {
            bind_params.push(Box::new(*conf));
        }
        if let Some(cert) = &filter.certainty {
            bind_params.push(Box::new(format!("{:?}", cert).to_lowercase()));
        }
        if let Some(topics) = &filter.topics {
            bind_params.push(Box::new(serde_json::to_string(topics)?));
        }
        bind_params.push(Box::new(limit as i64));

        let params: Vec<&dyn rusqlite::ToSql> = bind_params.iter().map(|b| b.as_ref()).collect();

        let facts = stmt
            .query_map(params.as_slice(), |row| self.row_to_fact(row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Load topics and evidence for each fact
        let mut result = Vec::new();
        for mut fact in facts {
            fact.topics = self.get_topics(fact.id)?;
            fact.evidence = self.get_evidence(fact.id)?;
            result.push(fact);
        }

        Ok(result)
    }

    /// Get all facts (with optional filter).
    pub fn list_facts(&self, filter: &FactFilter, limit: usize) -> Result<Vec<Fact>> {
        self.search("", filter, limit)
    }

    /// Get stale facts (optionally filtered by hours since last verification).
    pub fn get_stale_facts(&self, threshold_hours: Option<i64>) -> Result<Vec<Fact>> {
        let mut facts = Vec::new();

        if let Some(hours) = threshold_hours {
            let threshold = Utc::now() - chrono::Duration::hours(hours);
            let mut stmt = self.conn.prepare(
                r#"
                SELECT id, content, source, source_type, source_content_hash, git_commit,
                       confidence, certainty, created_at, last_verified, stale,
                       category, importance, scope, derived_from, supersedes,
                       access_count, last_accessed
                FROM facts
                WHERE last_verified < ?1 OR stale = 1
                "#,
            )?;

            let rows = stmt.query_map(params![threshold.to_rfc3339()], |row| {
                self.row_to_fact(row)
            })?;

            for row in rows {
                let mut fact = row?;
                fact.topics = self.get_topics(fact.id)?;
                fact.evidence = self.get_evidence(fact.id)?;
                facts.push(fact);
            }
        } else {
            let mut stmt = self.conn.prepare(
                r#"
                SELECT id, content, source, source_type, source_content_hash, git_commit,
                       confidence, certainty, created_at, last_verified, stale,
                       category, importance, scope, derived_from, supersedes,
                       access_count, last_accessed
                FROM facts WHERE stale = 1
                "#,
            )?;

            let rows = stmt.query_map([], |row| self.row_to_fact(row))?;

            for row in rows {
                let mut fact = row?;
                fact.topics = self.get_topics(fact.id)?;
                fact.evidence = self.get_evidence(fact.id)?;
                facts.push(fact);
            }
        }

        Ok(facts)
    }

    /// List all topics with counts.
    pub fn list_topics(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT topic, COUNT(*) as cnt FROM fact_topics GROUP BY topic ORDER BY cnt DESC",
        )?;

        let topics = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(topics)
    }

    // ========================================================================
    // Relations
    // ========================================================================

    /// Add a relation between facts.
    pub fn insert_relation(&self, relation: &Relation) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO relations (from_id, to_id, relation_type, created_at, metadata)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                relation.from_id.to_string(),
                relation.to_id.to_string(),
                format!("{:?}", relation.relation_type).to_lowercase(),
                relation.created_at.to_rfc3339(),
                relation.metadata,
            ],
        )?;
        Ok(())
    }

    /// Remove a relation.
    pub fn delete_relation(&self, from_id: Uuid, to_id: Uuid, relation_type: RelationType) -> Result<bool> {
        let rows = self.conn.execute(
            "DELETE FROM relations WHERE from_id = ?1 AND to_id = ?2 AND relation_type = ?3",
            params![
                from_id.to_string(),
                to_id.to_string(),
                format!("{:?}", relation_type).to_lowercase(),
            ],
        )?;
        Ok(rows > 0)
    }

    /// Get relations for a fact.
    pub fn get_relations(&self, fact_id: Uuid) -> Result<Vec<Relation>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT from_id, to_id, relation_type, created_at, metadata
            FROM relations
            WHERE from_id = ?1 OR to_id = ?1
            "#,
        )?;

        let relations = stmt
            .query_map(params![fact_id.to_string()], |row| {
                let from_id_str: String = row.get(0)?;
                let to_id_str: String = row.get(1)?;
                let created_at_str: String = row.get(3)?;

                let from_id = Uuid::parse_str(&from_id_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
                })?;
                let to_id = Uuid::parse_str(&to_id_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
                })?;
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
                    })?
                    .with_timezone(&Utc);

                Ok(Relation {
                    from_id,
                    to_id,
                    relation_type: parse_relation_type(&row.get::<_, String>(2)?),
                    created_at,
                    metadata: row.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(relations)
    }

    /// Find contradictions (facts with Contradicts relations).
    pub fn find_contradictions(&self) -> Result<Vec<(Fact, Fact, String)>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT from_id, to_id, metadata
            FROM relations
            WHERE relation_type = 'contradicts'
            "#,
        )?;

        let mut contradictions = Vec::new();

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            ))
        })?;

        for row in rows {
            let (from_id, to_id, reason) = row?;
            let from_uuid = Uuid::parse_str(&from_id).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
            })?;
            let to_uuid = Uuid::parse_str(&to_id).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
            })?;

            if let (Some(fact_a), Some(fact_b)) = (self.get_fact(from_uuid)?, self.get_fact(to_uuid)?) {
                contradictions.push((fact_a, fact_b, reason));
            }
        }

        Ok(contradictions)
    }

    // ========================================================================
    // Helper Methods
    // ========================================================================

    fn row_to_fact(&self, row: &rusqlite::Row) -> std::result::Result<Fact, rusqlite::Error> {
        Ok(Fact {
            id: Uuid::parse_str(&row.get::<_, String>(0)?).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
            })?,
            content: row.get(1)?,
            source: row.get(2)?,
            source_type: parse_source_type(&row.get::<_, String>(3)?),
            source_content_hash: row.get(4)?,
            git_commit: row.get(5)?,
            confidence: row.get(6)?,
            certainty: parse_certainty(&row.get::<_, String>(7)?),
            created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
                })?
                .with_timezone(&Utc),
            last_verified: DateTime::parse_from_rfc3339(&row.get::<_, String>(9)?)
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(e))
                })?
                .with_timezone(&Utc),
            stale: row.get::<_, i32>(10)? != 0,
            category: parse_category(&row.get::<_, String>(11)?),
            importance: parse_importance(&row.get::<_, String>(12)?),
            scope: parse_scope(&row.get::<_, String>(13)?),
            derived_from: row
                .get::<_, Option<String>>(14)?
                .and_then(|s| Uuid::parse_str(&s).ok()),
            supersedes: row
                .get::<_, Option<String>>(15)?
                .and_then(|s| Uuid::parse_str(&s).ok()),
            access_count: row.get(16)?,
            last_accessed: row
                .get::<_, Option<String>>(17)?
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|d| d.with_timezone(&Utc)),
            topics: Vec::new(),  // Loaded separately
            evidence: Vec::new(), // Loaded separately
        })
    }

    fn get_topics(&self, fact_id: Uuid) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT topic FROM fact_topics WHERE fact_id = ?1")?;
        let topics = stmt
            .query_map(params![fact_id.to_string()], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(topics)
    }

    fn get_evidence(&self, fact_id: Uuid) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT evidence FROM fact_evidence WHERE fact_id = ?1")?;
        let evidence = stmt
            .query_map(params![fact_id.to_string()], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(evidence)
    }

    /// Mark fact as accessed (increment counter, update timestamp).
    pub fn mark_accessed(&self, id: Uuid) -> Result<()> {
        self.conn.execute(
            "UPDATE facts SET access_count = access_count + 1, last_accessed = ?2 WHERE id = ?1",
            params![id.to_string(), Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Mark fact as stale.
    pub fn mark_stale(&self, id: Uuid, stale: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE facts SET stale = ?2 WHERE id = ?1",
            params![id.to_string(), stale as i32],
        )?;
        Ok(())
    }

    /// Update last_verified timestamp.
    pub fn mark_verified(&self, id: Uuid) -> Result<()> {
        self.conn.execute(
            "UPDATE facts SET last_verified = ?2, stale = 0 WHERE id = ?1",
            params![id.to_string(), Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Get total fact count.
    pub fn count_facts(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM facts", [], |row| row.get(0))?;
        Ok(count as usize)
    }
}

// ============================================================================
// Parsing helpers
// ============================================================================

fn parse_source_type(s: &str) -> SourceType {
    match s.to_lowercase().as_str() {
        "code" => SourceType::Code,
        "conversation" => SourceType::Conversation,
        "manual" => SourceType::Manual,
        "inferred" => SourceType::Inferred,
        _ => SourceType::Manual,
    }
}

fn parse_category(s: &str) -> Category {
    match s.to_lowercase().as_str() {
        "architecture" => Category::Architecture,
        "decision" => Category::Decision,
        "pattern" => Category::Pattern,
        "convention" => Category::Convention,
        "bug" => Category::Bug,
        "todo" => Category::Todo,
        "dependency" => Category::Dependency,
        "preference" => Category::Preference,
        "context" => Category::Context,
        _ => Category::Context,
    }
}

fn parse_importance(s: &str) -> Importance {
    match s.to_lowercase().as_str() {
        "critical" => Importance::Critical,
        "high" => Importance::High,
        "normal" => Importance::Normal,
        "low" => Importance::Low,
        _ => Importance::Normal,
    }
}

fn parse_certainty(s: &str) -> Certainty {
    match s.to_lowercase().as_str() {
        "definite" => Certainty::Definite,
        "likely" => Certainty::Likely,
        "uncertain" => Certainty::Uncertain,
        "speculative" => Certainty::Speculative,
        _ => Certainty::Likely,
    }
}

fn parse_scope(s: &str) -> Scope {
    match s.to_lowercase().as_str() {
        "global" => Scope::Global,
        "project" => Scope::Project,
        "branch" => Scope::Branch,
        "task" => Scope::Task,
        _ => Scope::Project,
    }
}

fn parse_relation_type(s: &str) -> RelationType {
    match s.to_lowercase().as_str() {
        "dependson" => RelationType::DependsOn,
        "contradicts" => RelationType::Contradicts,
        "elaborates" => RelationType::Elaborates,
        "relatedto" => RelationType::RelatedTo,
        "partof" => RelationType::PartOf,
        "supersedes" => RelationType::Supersedes,
        _ => RelationType::RelatedTo,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_crud() -> Result<()> {
        let storage = Storage::in_memory()?;

        // Create
        let fact = Fact::new("Test fact about authentication")
            .with_topics(vec!["auth".into(), "security".into()])
            .with_category(Category::Pattern)
            .with_importance(Importance::High);

        storage.insert_fact(&fact)?;

        // Read
        let loaded = storage.get_fact(fact.id)?.expect("fact should exist");
        assert_eq!(loaded.content, "Test fact about authentication");
        assert_eq!(loaded.topics, vec!["auth", "security"]);

        // Update
        let mut updated = loaded;
        updated.confidence = 0.95;
        storage.update_fact(&updated)?;

        let reloaded = storage.get_fact(fact.id)?.expect("fact should exist");
        assert!((reloaded.confidence - 0.95).abs() < 0.001);

        // Delete
        assert!(storage.delete_fact(fact.id)?);
        assert!(storage.get_fact(fact.id)?.is_none());

        Ok(())
    }

    #[test]
    fn test_search() -> Result<()> {
        let storage = Storage::in_memory()?;

        storage.insert_fact(&Fact::new("Authentication uses JWT tokens"))?;
        storage.insert_fact(&Fact::new("Database is PostgreSQL"))?;
        storage.insert_fact(&Fact::new("API uses REST authentication"))?;

        let results = storage.search("authentication", &FactFilter::default(), 10)?;
        assert_eq!(results.len(), 2);

        Ok(())
    }

    #[test]
    fn test_relations() -> Result<()> {
        let storage = Storage::in_memory()?;

        let fact_a = Fact::new("Fact A");
        let fact_b = Fact::new("Fact B");

        storage.insert_fact(&fact_a)?;
        storage.insert_fact(&fact_b)?;

        let relation = Relation::new(fact_a.id, fact_b.id, RelationType::DependsOn);
        storage.insert_relation(&relation)?;

        let relations = storage.get_relations(fact_a.id)?;
        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].relation_type, RelationType::DependsOn);

        Ok(())
    }
}
