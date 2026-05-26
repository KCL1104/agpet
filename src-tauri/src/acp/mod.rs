//! ACP client core (Milestone 1, steps 2–4).
//!
//! Spawns `@zed-industries/claude-code-acp`, completes the handshake, maps
//! `session/update` events to pet states + chat events, and (step 4) accepts
//! user prompts and routes permission decisions back to the agent.

mod client;
pub mod config;
pub mod logging;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::AppHandle;
use tokio::sync::{mpsc, oneshot};

pub use config::AcpConfig;

/// Commands sent into the live ACP connection from Tauri commands.
pub enum AcpCommand {
    /// Send a user prompt (`session/prompt`).
    Prompt(String),
}

/// Pending `session/request_permission` requests awaiting a user decision.
/// Keyed by request id; the value resolves with the chosen option id.
pub type PendingPermissions = Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>;

/// Current state of the ACP connection (precursor to the full pet state machine).
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

/// Tauri-managed handle to the ACP client. Holds the shared status, the command
/// channel into the live connection, and the pending-permission map. `Send + Sync`.
pub struct AcpManager {
    status: Arc<Mutex<AcpStatus>>,
    cmd_tx: mpsc::UnboundedSender<AcpCommand>,
    cmd_rx: Mutex<Option<mpsc::UnboundedReceiver<AcpCommand>>>,
    pending_permissions: PendingPermissions,
}

impl AcpManager {
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        Self {
            status: Arc::new(Mutex::new(AcpStatus::Idle)),
            cmd_tx,
            cmd_rx: Mutex::new(Some(cmd_rx)),
            pending_permissions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Snapshot of the current status (for the `acp_status` command).
    pub fn status(&self) -> AcpStatus {
        self.status
            .lock()
            .map(|g| g.clone())
            .unwrap_or(AcpStatus::Error {
                message: "status mutex poisoned".into(),
            })
    }

    /// Spawn the ACP adapter and run the handshake + command loop. Call once.
    pub fn start(&self, app: AppHandle, config: AcpConfig) {
        let cmd_rx = self
            .cmd_rx
            .lock()
            .ok()
            .and_then(|mut g| g.take());
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
        );
    }

    /// Queue a user prompt for the live session.
    pub fn send_prompt(&self, text: String) -> Result<(), String> {
        self.cmd_tx
            .send(AcpCommand::Prompt(text))
            .map_err(|_| "ACP connection is not running".to_string())
    }

    /// Resolve a pending permission request with the user's chosen option id.
    pub fn respond_permission(&self, id: String, choice: String) {
        if let Ok(mut map) = self.pending_permissions.lock() {
            if let Some(tx) = map.remove(&id) {
                let _ = tx.send(choice);
            }
        }
    }
}

impl Default for AcpManager {
    fn default() -> Self {
        Self::new()
    }
}
