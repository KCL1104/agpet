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

/// A persisted pet: the stable identity (UUID) the runtime instance binds to.
/// Decoupled from the ephemeral `instance_id` (a per-launch counter) so name,
/// position, and future game stats (xp/level/hunger…) survive restarts. A
/// `companion` is a durable user pet (keyed by type + workdir); a `worker` is a
/// transient pet spawned for a delegation/workflow that credits its parent.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct PetRow {
    pub pet_id: String,
    pub type_id: String,
    pub workdir: Option<String>,
    pub kind: String,
    pub parent_pet_id: Option<String>,
    pub display_name: Option<String>,
    pub handle: Option<String>,
    pub color: Option<String>,
    pub last_x: Option<f64>,
    pub custom_y: Option<f64>,
    pub xp: i64,
    pub level: i64,
    pub hunger: f64,
    pub energy: f64,
    pub happiness: f64,
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

        let db = Self { pool };
        db.migrate().await.context("run migrations")?;
        Ok(db)
    }

    /// Apply ordered, idempotent migrations tracked by `PRAGMA user_version`.
    /// Existing pre-versioning databases sit at version 0 but already have the
    /// v1 tables (created with IF NOT EXISTS), so re-applying v1 is harmless.
    async fn migrate(&self) -> anyhow::Result<()> {
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&self.pool)
            .await
            .context("read user_version")?;
        if version < 1 {
            for stmt in MIGRATION_V1 {
                sqlx::query(stmt).execute(&self.pool).await.context("migration v1")?;
            }
            sqlx::query("PRAGMA user_version = 1").execute(&self.pool).await?;
        }
        if version < 2 {
            for stmt in MIGRATION_V2 {
                sqlx::query(stmt).execute(&self.pool).await.context("migration v2")?;
            }
            sqlx::query("PRAGMA user_version = 2").execute(&self.pool).await?;
        }
        Ok(())
    }

    // --- Pet identity (Slice A: keystone) ---------------------------------

    /// All durable companion pets (for priming the in-memory identity map at
    /// startup). Workers are transient and intentionally excluded.
    pub async fn list_companions(&self) -> anyhow::Result<Vec<PetRow>> {
        let rows = sqlx::query_as::<_, PetRow>(
            "SELECT pet_id, type_id, workdir, kind, parent_pet_id, display_name, handle, color, last_x, custom_y, xp, level, hunger, energy, happiness
             FROM pets WHERE kind = 'companion'",
        )
        .fetch_all(&self.pool)
        .await
        .context("list companions")?;
        Ok(rows)
    }

    /// Insert a pet, or refresh only its identity/position fields on conflict —
    /// game columns (xp/level/hunger/…) are preserved across re-launches.
    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_pet_identity(
        &self,
        pet_id: &str,
        type_id: &str,
        workdir: Option<&str>,
        kind: &str,
        parent_pet_id: Option<&str>,
        display_name: Option<&str>,
        handle: Option<&str>,
        color: Option<&str>,
        last_x: Option<f64>,
        custom_y: Option<f64>,
    ) -> anyhow::Result<()> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO pets
                (pet_id, type_id, workdir, kind, parent_pet_id, display_name, handle, color, last_x, custom_y, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(pet_id) DO UPDATE SET
                workdir = excluded.workdir,
                display_name = excluded.display_name,
                handle = excluded.handle,
                color = excluded.color,
                last_x = excluded.last_x,
                custom_y = excluded.custom_y,
                updated_at = excluded.updated_at",
        )
        .bind(pet_id)
        .bind(type_id)
        .bind(workdir)
        .bind(kind)
        .bind(parent_pet_id)
        .bind(display_name)
        .bind(handle)
        .bind(color)
        .bind(last_x)
        .bind(custom_y)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("upsert pet")?;
        Ok(())
    }

    /// Persist a pet's display name + mention handle (on rename).
    pub async fn set_pet_name(&self, pet_id: &str, name: &str, handle: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE pets SET display_name = ?, handle = ?, updated_at = ? WHERE pet_id = ?")
            .bind(name)
            .bind(handle)
            .bind(now_ms())
            .bind(pet_id)
            .execute(&self.pool)
            .await
            .context("set pet name")?;
        Ok(())
    }

    /// Persist a pet's last drop position + roaming height (custom_y = -1 means
    /// "reset to the default baseline").
    pub async fn set_pet_position(&self, pet_id: &str, last_x: Option<f64>, custom_y: f64) -> anyhow::Result<()> {
        sqlx::query("UPDATE pets SET last_x = ?, custom_y = ?, updated_at = ? WHERE pet_id = ?")
            .bind(last_x)
            .bind(custom_y)
            .bind(now_ms())
            .bind(pet_id)
            .execute(&self.pool)
            .await
            .context("set pet position")?;
        Ok(())
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

/// Migration 1 — the original session schema (feature D). Idempotent so it can
/// also "adopt" pre-versioning databases that already have these tables.
const MIGRATION_V1: &[&str] = &[
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

/// Migration 2 — stable pet identity + an immutable XP ledger (keystone for the
/// production data model AND the pet game). `pets` holds durable companions and
/// transient workers; game columns (xp/level/hunger/energy/happiness) are seeded
/// with defaults now so adding the game layer needs no further migration. The
/// `xp_events` ledger is kept separate from (deletable) session content so a user
/// can purge transcripts without losing a pet's progression. owner_id/device_id
/// are reserved (unused) for a possible future multi-device/sync model.
const MIGRATION_V2: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS pets (
        pet_id TEXT PRIMARY KEY,
        type_id TEXT NOT NULL,
        workdir TEXT,
        kind TEXT NOT NULL DEFAULT 'companion',
        parent_pet_id TEXT,
        display_name TEXT,
        handle TEXT,
        color TEXT,
        last_x REAL,
        custom_y REAL,
        xp INTEGER NOT NULL DEFAULT 0,
        level INTEGER NOT NULL DEFAULT 1,
        hunger REAL NOT NULL DEFAULT 100,
        energy REAL NOT NULL DEFAULT 100,
        happiness REAL NOT NULL DEFAULT 100,
        last_fed_at INTEGER,
        last_decay_at INTEGER,
        owner_id TEXT,
        device_id TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_pets_companion ON pets(type_id, workdir, kind)",
    "CREATE TABLE IF NOT EXISTS xp_events (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        pet_id TEXT NOT NULL,
        kind TEXT NOT NULL,
        amount INTEGER NOT NULL,
        session_id TEXT,
        created_at INTEGER NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_xp_pet ON xp_events(pet_id)",
];
