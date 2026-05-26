//! ACP client core (Milestone 1–2).
//!
//! Spawns `@zed-industries/claude-code-acp`, completes the handshake, maps
//! `session/update` to pet states + chat events, accepts user prompts, routes
//! permission decisions, and (M2) persists each session to SQLite with an
//! agent-generated summary, supporting New / Resume.

mod client;
pub mod config;
pub mod logging;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::AppHandle;
use tokio::sync::{mpsc, oneshot};

pub use config::AcpConfig;

use crate::db::Db;

/// Commands sent into the live ACP connection from Tauri commands.
pub enum AcpCommand {
    /// Send a user prompt (`session/prompt`).
    Prompt(String),
    /// End + summarize the current session, then open a fresh one.
    NewSession,
    /// End + summarize the current session, open a fresh one, and seed it with a
    /// past session's summary (by DB id).
    Resume(String),
}

/// Pending `session/request_permission` requests awaiting a user decision.
pub type PendingPermissions = Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>;

/// Current state of the ACP connection.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AcpStatus {
    Idle,
    Connecting,
    Connected {
        session_id: String,
        agent: serde_json::Value,
    },
    AuthRequired { message: String },
    Error { message: String },
    Exited { message: String },
}

/// Tauri-managed handle to the ACP client.
pub struct AcpManager {
    status: Arc<Mutex<AcpStatus>>,
    cmd_tx: mpsc::UnboundedSender<AcpCommand>,
    cmd_rx: Mutex<Option<mpsc::UnboundedReceiver<AcpCommand>>>,
    pending_permissions: PendingPermissions,
    db: Arc<Db>,
}

impl AcpManager {
    pub fn new(db: Arc<Db>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        Self {
            status: Arc::new(Mutex::new(AcpStatus::Idle)),
            cmd_tx,
            cmd_rx: Mutex::new(Some(cmd_rx)),
            pending_permissions: Arc::new(Mutex::new(HashMap::new())),
            db,
        }
    }

    pub fn status(&self) -> AcpStatus {
        self.status
            .lock()
            .map(|g| g.clone())
            .unwrap_or(AcpStatus::Error { message: "status mutex poisoned".into() })
    }

    /// Spawn the ACP adapter and run the handshake + command loop. Call once.
    pub fn start(&self, app: AppHandle, config: AcpConfig) {
        let cmd_rx = self.cmd_rx.lock().ok().and_then(|mut g| g.take());
        let Some(cmd_rx) = cmd_rx else {
            tracing::error!("AcpManager::start called more than once");
            return;
        };
        client::start(
            app,
            config,
            self.status.clone(),
            cmd_rx,
            self.pending_permissions.clone(),
            self.db.clone(),
        );
    }

    pub fn send_prompt(&self, text: String) -> Result<(), String> {
        self.cmd_tx
            .send(AcpCommand::Prompt(text))
            .map_err(|_| "ACP connection is not running".to_string())
    }

    pub fn new_session(&self) -> Result<(), String> {
        self.cmd_tx
            .send(AcpCommand::NewSession)
            .map_err(|_| "ACP connection is not running".to_string())
    }

    pub fn resume_session(&self, id: String) -> Result<(), String> {
        self.cmd_tx
            .send(AcpCommand::Resume(id))
            .map_err(|_| "ACP connection is not running".to_string())
    }

    pub fn respond_permission(&self, id: String, choice: String) {
        if let Ok(mut map) = self.pending_permissions.lock() {
            if let Some(tx) = map.remove(&id) {
                let _ = tx.send(choice);
            }
        }
    }

    /// Clone of the DB handle, for read-only queries from async commands
    /// (so the `State` guard isn't held across an await).
    pub fn db_handle(&self) -> Arc<Db> {
        self.db.clone()
    }
}
