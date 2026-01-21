//! SQLite storage layer with FTS5 full-text search.

use crate::error::Result;
use crate::types::{
    Category, Certainty, Fact, FactFilter, FactHistoryEntry, Importance, Relation, RelationType,
    Scope, SourceType,
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

                -- Project context
                project_path TEXT,

                -- Session context
                session_id TEXT,

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
                archived INTEGER NOT NULL DEFAULT 0,

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

            -- Migration: Add archived column if missing (for existing databases)
            -- SQLite doesn't support IF NOT EXISTS for columns, so we check via pragma
            -- This is handled by the migration logic below

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

            -- Fact history for versioning
            CREATE TABLE IF NOT EXISTS fact_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                fact_id TEXT NOT NULL,
                version INTEGER NOT NULL,
                content TEXT NOT NULL,
                confidence REAL NOT NULL,
                changed_at TEXT NOT NULL,
                change_reason TEXT,
                FOREIGN KEY (fact_id) REFERENCES facts(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_fact_history_fact_id ON fact_history(fact_id);

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
            CREATE INDEX IF NOT EXISTS idx_facts_archived ON facts(archived);
            CREATE INDEX IF NOT EXISTS idx_facts_project ON facts(project_path);
            CREATE INDEX IF NOT EXISTS idx_facts_session ON facts(session_id);
            "#,
        )?;

        // Migrations for existing databases
        self.migrate_add_archived_column()?;
        self.migrate_add_project_path_column()?;
        self.migrate_add_session_id_column()?;

        Ok(())
    }

    // ========================================================================
    // Migrations
    // ========================================================================

    /// Migration: Add archived column if it doesn't exist (for existing databases).
    fn migrate_add_archived_column(&self) -> Result<()> {
        let columns = self.get_table_columns("facts")?;
        if !columns.contains(&"archived".to_string()) {
            self.conn.execute(
                "ALTER TABLE facts ADD COLUMN archived INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        Ok(())
    }

    /// Migration: Add project_path column if it doesn't exist (for existing databases).
    fn migrate_add_project_path_column(&self) -> Result<()> {
        let columns = self.get_table_columns("facts")?;
        if !columns.contains(&"project_path".to_string()) {
            self.conn.execute(
                "ALTER TABLE facts ADD COLUMN project_path TEXT",
                [],
            )?;
        }
        Ok(())
    }

    /// Migration: Add session_id column if it doesn't exist (for existing databases).
    fn migrate_add_session_id_column(&self) -> Result<()> {
        let columns = self.get_table_columns("facts")?;
        if !columns.contains(&"session_id".to_string()) {
            self.conn.execute(
                "ALTER TABLE facts ADD COLUMN session_id TEXT",
                [],
            )?;
        }
        Ok(())
    }

    /// Helper: Get column names for a table.
    fn get_table_columns(&self, table: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({})", table))?;
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(columns)
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
                id, content, project_path, session_id, source, source_type, source_content_hash, git_commit,
                confidence, certainty, created_at, last_verified, stale,
                category, importance, scope, derived_from, supersedes,
                access_count, last_accessed
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20
            )
            "#,
            params![
                fact.id.to_string(),
                fact.content,
                fact.project_path,
                fact.session_id,
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
            SELECT id, content, project_path, session_id, source, source_type, source_content_hash, git_commit,
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

    /// Update a fact, saving the previous version to history.
    pub fn update_fact(&self, fact: &Fact) -> Result<()> {
        self.update_fact_with_reason(fact, None)
    }

    /// Update a fact with a reason for the change.
    pub fn update_fact_with_reason(&self, fact: &Fact, change_reason: Option<&str>) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        // Get current version for history
        if let Some(old_fact) = self.get_fact_internal(&tx, fact.id)? {
            // Get the next version number
            let version: i64 = tx
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) + 1 FROM fact_history WHERE fact_id = ?1",
                    params![fact.id.to_string()],
                    |row| row.get(0),
                )
                .unwrap_or(1);

            // Save old version to history
            tx.execute(
                r#"
                INSERT INTO fact_history (fact_id, version, content, confidence, changed_at, change_reason)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![
                    fact.id.to_string(),
                    version,
                    old_fact.content,
                    old_fact.confidence,
                    Utc::now().to_rfc3339(),
                    change_reason,
                ],
            )?;
        }

        tx.execute(
            r#"
            UPDATE facts SET
                content = ?2, project_path = ?3, source = ?4, source_type = ?5, source_content_hash = ?6,
                git_commit = ?7, confidence = ?8, certainty = ?9, last_verified = ?10,
                stale = ?11, category = ?12, importance = ?13, scope = ?14,
                derived_from = ?15, supersedes = ?16, access_count = ?17, last_accessed = ?18
            WHERE id = ?1
            "#,
            params![
                fact.id.to_string(),
                fact.content,
                fact.project_path,
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

    /// Internal method to get a fact within a transaction.
    fn get_fact_internal(&self, conn: &Connection, id: Uuid) -> Result<Option<Fact>> {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, content, project_path, session_id, source, source_type, source_content_hash, git_commit,
                   confidence, certainty, created_at, last_verified, stale,
                   category, importance, scope, derived_from, supersedes,
                   access_count, last_accessed
            FROM facts WHERE id = ?1
            "#,
        )?;

        let fact = stmt
            .query_row(params![id.to_string()], |row| self.row_to_fact(row))
            .optional()?;

        Ok(fact)
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
    /// Uses synonym expansion for better recall.
    /// If project_path is provided in filter, searches only that project.
    pub fn search(&self, query: &str, filter: &FactFilter, limit: usize) -> Result<Vec<Fact>> {
        let use_fts = !query.trim().is_empty();

        // Build FTS5 query (sanitized for safe matching)
        let fts_query = if use_fts {
            sanitize_fts5_query(query)
        } else {
            String::new()
        };

        // Tokenize query for topic matching
        let query_terms = crate::utils::tokenize_query(query);

        // Build the query using FTS5 for fast content search
        // Use UNION to combine FTS5 content matches with topic matches
        let mut sql = if use_fts && !query_terms.is_empty() {
            format!(
                r#"
                WITH matched_ids AS (
                    -- FTS5 content matches
                    SELECT f.rowid as fact_rowid
                    FROM facts f
                    JOIN facts_fts fts ON f.rowid = fts.rowid
                    WHERE facts_fts MATCH ?
                    UNION
                    -- Topic matches (exact match on query terms)
                    SELECT f.rowid as fact_rowid
                    FROM facts f
                    JOIN fact_topics ft ON f.id = ft.fact_id
                    WHERE ft.topic IN ({})
                )
                SELECT DISTINCT f.id, f.content, f.project_path, f.session_id, f.source, f.source_type, f.source_content_hash,
                       f.git_commit, f.confidence, f.certainty, f.created_at, f.last_verified,
                       f.stale, f.category, f.importance, f.scope, f.derived_from, f.supersedes,
                       f.access_count, f.last_accessed,
                       (f.confidence * CASE f.importance WHEN 'critical' THEN 4.0 WHEN 'high' THEN 2.0 WHEN 'normal' THEN 1.0 ELSE 0.5 END) as score
                FROM facts f
                JOIN matched_ids m ON f.rowid = m.fact_rowid
                "#,
                query_terms.iter().map(|_| "?").collect::<Vec<_>>().join(",")
            )
        } else if use_fts {
            // Query has no tokenizable terms, just use FTS5
            String::from(
                r#"
                SELECT DISTINCT f.id, f.content, f.project_path, f.session_id, f.source, f.source_type, f.source_content_hash,
                       f.git_commit, f.confidence, f.certainty, f.created_at, f.last_verified,
                       f.stale, f.category, f.importance, f.scope, f.derived_from, f.supersedes,
                       f.access_count, f.last_accessed,
                       (f.confidence * CASE f.importance WHEN 'critical' THEN 4.0 WHEN 'high' THEN 2.0 WHEN 'normal' THEN 1.0 ELSE 0.5 END) as score
                FROM facts f
                JOIN facts_fts fts ON f.rowid = fts.rowid
                WHERE facts_fts MATCH ?
                "#,
            )
        } else {
            String::from(
                r#"
                SELECT DISTINCT f.id, f.content, f.project_path, f.session_id, f.source, f.source_type, f.source_content_hash,
                       f.git_commit, f.confidence, f.certainty, f.created_at, f.last_verified,
                       f.stale, f.category, f.importance, f.scope, f.derived_from, f.supersedes,
                       f.access_count, f.last_accessed,
                       (f.confidence * CASE f.importance WHEN 'critical' THEN 4.0 WHEN 'high' THEN 2.0 WHEN 'normal' THEN 1.0 ELSE 0.5 END) as score
                FROM facts f
                "#,
            )
        };

        let mut conditions: Vec<String> = Vec::new();
        conditions.push("f.archived = 0".to_string());

        // Project filtering
        let project_filter = if filter.all_projects {
            None
        } else {
            filter.project_path.clone().or_else(crate::utils::get_project_root)
        };
        let use_project_filter = project_filter.is_some();
        if use_project_filter {
            conditions.push("(f.project_path = ? OR f.project_path IS NULL)".to_string());
        }

        // Session filtering
        let use_session_filter = filter.session_id.is_some();
        if use_session_filter {
            conditions.push("f.session_id = ?".to_string());
        }

        if filter.category.is_some() {
            conditions.push("f.category = ?".to_string());
        }
        if filter.importance.is_some() {
            conditions.push("f.importance = ?".to_string());
        }
        if filter.scope.is_some() {
            conditions.push("f.scope = ?".to_string());
        }
        if filter.source_type.is_some() {
            conditions.push("f.source_type = ?".to_string());
        }
        if filter.stale.is_some() {
            conditions.push("f.stale = ?".to_string());
        }
        if filter.min_confidence.is_some() {
            conditions.push("f.confidence >= ?".to_string());
        }
        if filter.certainty.is_some() {
            conditions.push("f.certainty = ?".to_string());
        }
        if filter.topics.is_some() {
            sql.push_str("JOIN fact_topics ft ON f.id = ft.fact_id ");
            conditions.push("ft.topic IN (SELECT value FROM json_each(?))".to_string());
        }

        if !conditions.is_empty() {
            sql.push_str("WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }

        sql.push_str(" ORDER BY score DESC, f.importance DESC LIMIT ?");

        let mut stmt = self.conn.prepare(&sql)?;

        let mut bind_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        // FTS5 query and topic terms come first (for the CTE)
        if use_fts {
            bind_params.push(Box::new(fts_query.clone()));
            // Add each query term for the topic IN clause
            for term in &query_terms {
                bind_params.push(Box::new(term.clone()));
            }
        }

        // Project filter
        if let Some(project) = project_filter {
            bind_params.push(Box::new(project));
        }
        // Session filter
        if let Some(session_id) = &filter.session_id {
            bind_params.push(Box::new(session_id.clone()));
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

        let mut facts: Vec<Fact> = stmt
            .query_map(params.as_slice(), |row| self.row_to_fact(row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Batch load topics and evidence (avoids N+1 query problem)
        if !facts.is_empty() {
            let fact_ids: Vec<String> = facts.iter().map(|f| f.id.to_string()).collect();

            // Batch load topics
            let topics_map = self.get_topics_batch(&fact_ids)?;

            // Batch load evidence
            let evidence_map = self.get_evidence_batch(&fact_ids)?;

            // Assign to facts
            for fact in &mut facts {
                let id_str = fact.id.to_string();
                if let Some(topics) = topics_map.get(&id_str) {
                    fact.topics = topics.clone();
                }
                if let Some(evidence) = evidence_map.get(&id_str) {
                    fact.evidence = evidence.clone();
                }
            }
        }

        Ok(facts)
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
                SELECT id, content, project_path, session_id, source, source_type, source_content_hash, git_commit,
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
                SELECT id, content, project_path, session_id, source, source_type, source_content_hash, git_commit,
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

    /// Get facts grouped by category with counts.
    pub fn get_facts_by_category(
        &self,
        category: Option<Category>,
        limit_per_category: usize,
    ) -> Result<Vec<(Category, Vec<Fact>, Vec<String>)>> {
        let categories: Vec<Category> = if let Some(cat) = category {
            vec![cat]
        } else {
            // All categories
            vec![
                Category::Architecture,
                Category::Decision,
                Category::Pattern,
                Category::Convention,
                Category::Bug,
                Category::Todo,
                Category::Dependency,
                Category::Preference,
                Category::Context,
            ]
        };

        let mut results = Vec::new();

        for cat in categories {
            let cat_str = format!("{:?}", cat).to_lowercase();

            // Get facts for this category
            let mut stmt = self.conn.prepare(
                r#"
                SELECT id, content, project_path, session_id, source, source_type, source_content_hash,
                       git_commit, confidence, certainty, created_at, last_verified, stale,
                       category, importance, scope, derived_from, supersedes, access_count, last_accessed
                FROM facts
                WHERE category = ?1 AND archived = 0
                ORDER BY importance DESC, confidence DESC
                LIMIT ?2
                "#,
            )?;

            let facts: Vec<Fact> = stmt
                .query_map(params![cat_str, limit_per_category as i64], |row| {
                    self.row_to_fact(row)
                })?
                .filter_map(|r| r.ok())
                .collect();

            if facts.is_empty() {
                continue;
            }

            // Load topics for each fact and collect top topics
            let mut topic_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            let mut facts_with_topics = Vec::new();

            for mut fact in facts {
                fact.topics = self.get_topics(fact.id)?;
                fact.evidence = self.get_evidence(fact.id)?;
                for topic in &fact.topics {
                    *topic_counts.entry(topic.clone()).or_insert(0) += 1;
                }
                facts_with_topics.push(fact);
            }

            // Get top 5 topics
            let mut topic_vec: Vec<_> = topic_counts.into_iter().collect();
            topic_vec.sort_by(|a, b| b.1.cmp(&a.1));
            let top_topics: Vec<String> = topic_vec.into_iter().take(5).map(|(t, _)| t).collect();

            results.push((cat, facts_with_topics, top_topics));
        }

        Ok(results)
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
            project_path: row.get(2)?,
            session_id: row.get(3)?,
            source: row.get(4)?,
            source_type: parse_source_type(&row.get::<_, String>(5)?),
            source_content_hash: row.get(6)?,
            git_commit: row.get(7)?,
            confidence: row.get(8)?,
            certainty: parse_certainty(&row.get::<_, String>(9)?),
            created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(10)?)
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(10, rusqlite::types::Type::Text, Box::new(e))
                })?
                .with_timezone(&Utc),
            last_verified: DateTime::parse_from_rfc3339(&row.get::<_, String>(11)?)
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(11, rusqlite::types::Type::Text, Box::new(e))
                })?
                .with_timezone(&Utc),
            stale: row.get::<_, i32>(12)? != 0,
            category: parse_category(&row.get::<_, String>(13)?),
            importance: parse_importance(&row.get::<_, String>(14)?),
            scope: parse_scope(&row.get::<_, String>(15)?),
            derived_from: row
                .get::<_, Option<String>>(16)?
                .and_then(|s| Uuid::parse_str(&s).ok()),
            supersedes: row
                .get::<_, Option<String>>(17)?
                .and_then(|s| Uuid::parse_str(&s).ok()),
            access_count: row.get(18)?,
            last_accessed: row
                .get::<_, Option<String>>(19)?
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

    /// Batch load topics for multiple facts (avoids N+1 queries).
    fn get_topics_batch(&self, fact_ids: &[String]) -> Result<std::collections::HashMap<String, Vec<String>>> {
        use std::collections::HashMap;

        if fact_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let placeholders = fact_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("SELECT fact_id, topic FROM fact_topics WHERE fact_id IN ({})", placeholders);
        let mut stmt = self.conn.prepare(&sql)?;

        let params: Vec<&dyn rusqlite::ToSql> = fact_ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

        let mut result: HashMap<String, Vec<String>> = HashMap::new();
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        for row in rows {
            let (fact_id, topic) = row?;
            result.entry(fact_id).or_default().push(topic);
        }

        Ok(result)
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

    /// Batch load evidence for multiple facts (avoids N+1 queries).
    fn get_evidence_batch(&self, fact_ids: &[String]) -> Result<std::collections::HashMap<String, Vec<String>>> {
        use std::collections::HashMap;

        if fact_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let placeholders = fact_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("SELECT fact_id, evidence FROM fact_evidence WHERE fact_id IN ({})", placeholders);
        let mut stmt = self.conn.prepare(&sql)?;

        let params: Vec<&dyn rusqlite::ToSql> = fact_ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

        let mut result: HashMap<String, Vec<String>> = HashMap::new();
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        for row in rows {
            let (fact_id, evidence) = row?;
            result.entry(fact_id).or_default().push(evidence);
        }

        Ok(result)
    }

    /// Mark fact as accessed (increment counter, update timestamp, boost confidence).
    ///
    /// Confidence is boosted by 2% per access (capped at 1.0).
    /// This reinforces frequently-used facts.
    pub fn mark_accessed(&self, id: Uuid) -> Result<()> {
        self.conn.execute(
            r#"
            UPDATE facts SET
                access_count = access_count + 1,
                last_accessed = ?2,
                confidence = MIN(1.0, confidence + 0.02)
            WHERE id = ?1
            "#,
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
            .query_row("SELECT COUNT(*) FROM facts WHERE archived = 0", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    // ========================================================================
    // Memory Maintenance
    // ========================================================================

    /// Archive a fact (soft-delete).
    pub fn archive_fact(&self, id: Uuid) -> Result<bool> {
        let rows = self.conn.execute(
            "UPDATE facts SET archived = 1 WHERE id = ?1 AND archived = 0",
            params![id.to_string()],
        )?;
        Ok(rows > 0)
    }

    /// Unarchive a fact.
    pub fn unarchive_fact(&self, id: Uuid) -> Result<bool> {
        let rows = self.conn.execute(
            "UPDATE facts SET archived = 0 WHERE id = ?1 AND archived = 1",
            params![id.to_string()],
        )?;
        Ok(rows > 0)
    }

    /// Apply time-based confidence decay to facts not accessed recently.
    ///
    /// Formula: new_confidence = old_confidence * decay_factor
    /// Only affects facts not accessed in `threshold_days` days.
    ///
    /// Returns (count_affected, total_confidence_reduction).
    pub fn apply_decay(&self, threshold_days: i64, decay_factor: f32) -> Result<(usize, f32)> {
        let threshold = Utc::now() - chrono::Duration::days(threshold_days);
        let threshold_str = threshold.to_rfc3339();

        // Get facts eligible for decay (not accessed recently, not archived)
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, confidence FROM facts
            WHERE archived = 0
            AND (last_accessed IS NULL OR last_accessed < ?1)
            AND confidence > 0.1
            "#,
        )?;

        let facts: Vec<(String, f32)> = stmt
            .query_map(params![threshold_str], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f32>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut total_reduction = 0.0f32;
        let count = facts.len();

        for (id, old_confidence) in facts {
            let new_confidence = (old_confidence * decay_factor).max(0.1); // Floor at 0.1
            total_reduction += old_confidence - new_confidence;

            self.conn.execute(
                "UPDATE facts SET confidence = ?2 WHERE id = ?1",
                params![id, new_confidence],
            )?;
        }

        Ok((count, total_reduction))
    }

    /// Prune (archive or delete) old, unused, low-confidence facts.
    ///
    /// Returns list of affected fact IDs.
    pub fn prune_facts(
        &self,
        days_unused: i64,
        min_confidence: f32,
        archive: bool,
    ) -> Result<Vec<Uuid>> {
        let threshold = Utc::now() - chrono::Duration::days(days_unused);
        let threshold_str = threshold.to_rfc3339();

        // Find facts to prune
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id FROM facts
            WHERE archived = 0
            AND confidence < ?1
            AND (last_accessed IS NULL OR last_accessed < ?2)
            AND importance NOT IN ('critical', 'high')
            "#,
        )?;

        let fact_ids: Vec<Uuid> = stmt
            .query_map(params![min_confidence, threshold_str], |row| {
                let id_str: String = row.get(0)?;
                Uuid::parse_str(&id_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Archive or delete
        for id in &fact_ids {
            if archive {
                self.archive_fact(*id)?;
            } else {
                self.delete_fact(*id)?;
            }
        }

        Ok(fact_ids)
    }

    /// Find similar facts based on shared topics.
    ///
    /// Returns pairs of facts with similarity scores (0.0-1.0 based on topic overlap).
    pub fn find_similar_facts(&self, min_similarity: f32) -> Result<Vec<(Fact, Fact, f32)>> {
        // Find fact pairs that share topics
        let mut stmt = self.conn.prepare(
            r#"
            SELECT DISTINCT
                f1.id as id1,
                f2.id as id2,
                COUNT(DISTINCT t1.topic) as shared_topics,
                (SELECT COUNT(*) FROM fact_topics WHERE fact_id = f1.id) as total1,
                (SELECT COUNT(*) FROM fact_topics WHERE fact_id = f2.id) as total2
            FROM facts f1
            JOIN facts f2 ON f1.id < f2.id
            JOIN fact_topics t1 ON f1.id = t1.fact_id
            JOIN fact_topics t2 ON f2.id = t2.fact_id AND t1.topic = t2.topic
            WHERE f1.archived = 0 AND f2.archived = 0
            GROUP BY f1.id, f2.id
            HAVING shared_topics >= 2
            "#,
        )?;

        let pairs: Vec<(String, String, i64, i64, i64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut results = Vec::new();
        for (id1, id2, shared, total1, total2) in pairs {
            // Jaccard similarity: intersection / union
            let union = total1 + total2 - shared;
            let similarity = if union > 0 {
                shared as f32 / union as f32
            } else {
                0.0
            };

            if similarity >= min_similarity {
                let uuid1 = Uuid::parse_str(&id1).map_err(|e| {
                    crate::error::MemoryError::InvalidFilter(format!("Invalid UUID: {}", e))
                })?;
                let uuid2 = Uuid::parse_str(&id2).map_err(|e| {
                    crate::error::MemoryError::InvalidFilter(format!("Invalid UUID: {}", e))
                })?;

                if let (Some(fact1), Some(fact2)) = (self.get_fact(uuid1)?, self.get_fact(uuid2)?) {
                    results.push((fact1, fact2, similarity));
                }
            }
        }

        // Sort by similarity descending
        results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        Ok(results)
    }

    /// Find facts with overlapping topics for contradiction detection.
    ///
    /// Returns facts that share at least `min_overlap_ratio` of topics with the given topics.
    /// For example, if min_overlap_ratio is 0.5 and new_topics has 4 topics,
    /// returns facts that share at least 2 topics.
    pub fn find_facts_by_topic_overlap(
        &self,
        new_topics: &[String],
        min_overlap_ratio: f32,
        limit: usize,
    ) -> Result<Vec<(Fact, f32)>> {
        if new_topics.is_empty() {
            return Ok(Vec::new());
        }

        // Get all facts that have at least one of the topics
        let topics_json = serde_json::to_string(new_topics)?;
        let mut stmt = self.conn.prepare(
            r#"
            SELECT DISTINCT f.id, f.content, f.project_path, f.session_id, f.source, f.source_type,
                   f.source_content_hash, f.git_commit, f.confidence, f.certainty,
                   f.created_at, f.last_verified, f.stale, f.category, f.importance,
                   f.scope, f.derived_from, f.supersedes, f.access_count, f.last_accessed
            FROM facts f
            JOIN fact_topics ft ON f.id = ft.fact_id
            WHERE f.archived = 0
            AND ft.topic IN (SELECT value FROM json_each(?1))
            LIMIT ?2
            "#,
        )?;

        let facts: Vec<Fact> = stmt
            .query_map(params![topics_json, limit as i64], |row| self.row_to_fact(row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Calculate overlap ratio for each fact
        let mut results = Vec::new();
        let new_topics_set: std::collections::HashSet<_> = new_topics.iter().collect();

        for mut fact in facts {
            fact.topics = self.get_topics(fact.id)?;
            fact.evidence = self.get_evidence(fact.id)?;

            let fact_topics_set: std::collections::HashSet<_> = fact.topics.iter().collect();
            let intersection_count = new_topics_set.intersection(&fact_topics_set).count();

            // Jaccard-like: intersection / min(new_topics, fact_topics)
            let min_size = new_topics_set.len().min(fact_topics_set.len());
            let overlap_ratio = if min_size > 0 {
                intersection_count as f32 / min_size as f32
            } else {
                0.0
            };

            if overlap_ratio >= min_overlap_ratio {
                results.push((fact, overlap_ratio));
            }
        }

        // Sort by overlap ratio descending
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        Ok(results)
    }

    /// Get the history of changes for a fact.
    pub fn get_fact_history(&self, fact_id: Uuid) -> Result<Vec<FactHistoryEntry>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT version, content, confidence, changed_at, change_reason
            FROM fact_history
            WHERE fact_id = ?1
            ORDER BY version DESC
            "#,
        )?;

        let entries = stmt
            .query_map(params![fact_id.to_string()], |row| {
                let changed_at_str: String = row.get(3)?;
                Ok(FactHistoryEntry {
                    version: row.get(0)?,
                    content: row.get(1)?,
                    confidence: row.get(2)?,
                    changed_at: DateTime::parse_from_rfc3339(&changed_at_str)
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                3,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?
                        .with_timezone(&Utc),
                    change_reason: row.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    /// Get archived facts.
    pub fn get_archived_facts(&self, limit: usize) -> Result<Vec<Fact>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, content, project_path, session_id, source, source_type, source_content_hash, git_commit,
                   confidence, certainty, created_at, last_verified, stale,
                   category, importance, scope, derived_from, supersedes,
                   access_count, last_accessed
            FROM facts WHERE archived = 1
            ORDER BY last_accessed DESC NULLS LAST
            LIMIT ?1
            "#,
        )?;

        let facts = stmt
            .query_map(params![limit as i64], |row| self.row_to_fact(row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut result = Vec::new();
        for mut fact in facts {
            fact.topics = self.get_topics(fact.id)?;
            fact.evidence = self.get_evidence(fact.id)?;
            result.push(fact);
        }

        Ok(result)
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
