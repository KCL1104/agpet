//! SQLite persistence for session artifacts (Milestone 2, feature D).
//!
//! Schema is kept network-friendly (spec feature B prep, but no sync built):
//! UUID text ids, UTC unix-ms timestamps, no SQLite-specific column types — so a
//! future move to Postgres is not blocked. Runtime queries only (no compile-time
//! `query!` macros), so no database is needed at build time.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use uuid::Uuid;

/// A persisted session row, as returned to the history UI.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct SessionRow {
    pub id: String,
    pub agent_id: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub status: String,
    pub workdir: Option<String>,
    pub initial_prompt: Option<String>,
    pub summary: Option<String>,
    pub parent_session_id: Option<String>,
}

/// Owns the SQLite connection pool.
pub struct Db {
    pool: SqlitePool,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Db {
    /// Open (creating if needed) the database at `path` and ensure the schema.
    pub async fn init(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("create db dir")?;
        }
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .context("open sqlite pool")?;

        for stmt in SCHEMA {
            sqlx::query(stmt).execute(&pool).await.context("create schema")?;
        }
        Ok(Self { pool })
    }

    /// Insert a new active session; returns its UUID.
    pub async fn create_session(
        &self,
        agent_id: &str,
        workdir: Option<&str>,
        initial_prompt: Option<&str>,
        parent_session_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO sessions (id, agent_id, started_at, status, workdir, initial_prompt, parent_session_id)
             VALUES (?, ?, ?, 'active', ?, ?, ?)",
        )
        .bind(&id)
        .bind(agent_id)
        .bind(now_ms())
        .bind(workdir)
        .bind(initial_prompt)
        .bind(parent_session_id)
        .execute(&self.pool)
        .await
        .context("insert session")?;
        Ok(id)
    }

    /// Set the initial prompt if it is not already recorded (first user message).
    pub async fn set_initial_prompt(&self, session_id: &str, prompt: &str) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE sessions SET initial_prompt = ? WHERE id = ? AND (initial_prompt IS NULL OR initial_prompt = '')",
        )
        .bind(prompt)
        .bind(session_id)
        .execute(&self.pool)
        .await
        .context("set initial prompt")?;
        Ok(())
    }

    pub async fn append_event(
        &self,
        session_id: &str,
        event_type: &str,
        payload_json: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO session_events (session_id, timestamp, event_type, payload_json)
             VALUES (?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind(now_ms())
        .bind(event_type)
        .bind(payload_json)
        .execute(&self.pool)
        .await
        .context("append event")?;
        Ok(())
    }

    pub async fn append_file(
        &self,
        session_id: &str,
        file_path: &str,
        operation: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO session_files (session_id, file_path, operation)
             VALUES (?, ?, ?)",
        )
        .bind(session_id)
        .bind(file_path)
        .bind(operation)
        .execute(&self.pool)
        .await
        .context("append file")?;
        Ok(())
    }

    /// Mark a session finished with an optional summary.
    pub async fn finish_session(
        &self,
        session_id: &str,
        summary: Option<&str>,
        status: &str,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE sessions SET ended_at = ?, summary = ?, status = ? WHERE id = ?")
            .bind(now_ms())
            .bind(summary)
            .bind(status)
            .bind(session_id)
            .execute(&self.pool)
            .await
            .context("finish session")?;
        Ok(())
    }

    /// Most recent sessions for one agent (for its history panel).
    pub async fn list_sessions(&self, agent_id: &str, limit: i64) -> anyhow::Result<Vec<SessionRow>> {
        let rows = sqlx::query_as::<_, SessionRow>(
            "SELECT id, agent_id, started_at, ended_at, status, workdir, initial_prompt, summary, parent_session_id
             FROM sessions WHERE agent_id = ? ORDER BY started_at DESC LIMIT ?",
        )
        .bind(agent_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("list sessions")?;
        Ok(rows)
    }

    pub async fn get_session(&self, id: &str) -> anyhow::Result<Option<SessionRow>> {
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT id, agent_id, started_at, ended_at, status, workdir, initial_prompt, summary, parent_session_id
             FROM sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("get session")?;
        Ok(row)
    }
}

const SCHEMA: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS sessions (
        id TEXT PRIMARY KEY,
        agent_id TEXT NOT NULL,
        started_at INTEGER NOT NULL,
        ended_at INTEGER,
        status TEXT NOT NULL,
        workdir TEXT,
        initial_prompt TEXT,
        summary TEXT,
        workflow_id TEXT,
        parent_session_id TEXT,
        metadata_json TEXT
    )",
    "CREATE TABLE IF NOT EXISTS session_events (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id TEXT NOT NULL,
        timestamp INTEGER NOT NULL,
        event_type TEXT NOT NULL,
        payload_json TEXT,
        FOREIGN KEY(session_id) REFERENCES sessions(id)
    )",
    "CREATE TABLE IF NOT EXISTS session_files (
        session_id TEXT NOT NULL,
        file_path TEXT NOT NULL,
        operation TEXT NOT NULL,
        PRIMARY KEY (session_id, file_path, operation)
    )",
    "CREATE INDEX IF NOT EXISTS idx_events_session ON session_events(session_id)",
];
