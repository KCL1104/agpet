//! ACP client core (Milestone 1–3).
//!
//! Manages one ACP connection per declared agent (a pet). Each agent spawns its
//! own adapter, has its own status / command channel / pending-permission map,
//! and persists its own sessions (the DB is shared, keyed by agent_id).

mod client;
pub mod config;
pub mod logging;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::AppHandle;
use tokio::sync::{mpsc, oneshot};

pub use config::{AgentDef, AgentsConfig};

use crate::db::Db;

/// Commands sent into a live ACP connection from Tauri commands.
pub enum AcpCommand {
    Prompt(String),
    NewSession,
    Resume(String),
}

/// Pending `session/request_permission` requests awaiting a user decision.
pub type PendingPermissions = Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>;

/// Current state of one agent's connection.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AcpStatus {
    Idle,
    Connecting,
    Connected { session_id: String },
    AuthRequired { message: String },
    Error { message: String },
    Exited { message: String },
}

/// Minimal agent info for the frontend (to render pets).
#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub color: String,
}

/// Per-agent runtime handle.
struct AgentHandle {
    def: AgentDef,
    status: Arc<Mutex<AcpStatus>>,
    cmd_tx: mpsc::UnboundedSender<AcpCommand>,
    cmd_rx: Mutex<Option<mpsc::UnboundedReceiver<AcpCommand>>>,
    pending: PendingPermissions,
}

/// Tauri-managed handle to all agents.
pub struct AcpManager {
    agents: Vec<AgentHandle>,
    db: Arc<Db>,
    cwd: PathBuf,
    log_dir: PathBuf,
}

impl AcpManager {
    pub fn new(db: Arc<Db>, config: &AgentsConfig) -> Self {
        let agents = config
            .agents
            .iter()
            .map(|def| {
                let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
                AgentHandle {
                    def: def.clone(),
                    status: Arc::new(Mutex::new(AcpStatus::Idle)),
                    cmd_tx,
                    cmd_rx: Mutex::new(Some(cmd_rx)),
                    pending: Arc::new(Mutex::new(HashMap::new())),
                }
            })
            .collect();
        Self {
            agents,
            db,
            cwd: config.cwd.clone(),
            log_dir: config.log_dir.clone(),
        }
    }

    /// Spawn every agent's adapter + handshake + command loop. Call once.
    pub fn start_all(&self, app: AppHandle) {
        for h in &self.agents {
            let Some(cmd_rx) = h.cmd_rx.lock().ok().and_then(|mut g| g.take()) else {
                continue;
            };
            let log_path = self.log_dir.join(format!("acp-messages-{}.jsonl", h.def.id));
            client::start(
                app.clone(),
                h.def.clone(),
                self.cwd.clone(),
                log_path,
                h.status.clone(),
                cmd_rx,
                h.pending.clone(),
                self.db.clone(),
            );
        }
    }

    pub fn list_agents(&self) -> Vec<AgentInfo> {
        self.agents
            .iter()
            .map(|h| AgentInfo {
                id: h.def.id.clone(),
                name: h.def.name.clone(),
                color: h.def.color.clone(),
            })
            .collect()
    }

    fn agent(&self, id: &str) -> Option<&AgentHandle> {
        self.agents.iter().find(|h| h.def.id == id)
    }

    fn send(&self, agent_id: &str, cmd: AcpCommand) -> Result<(), String> {
        let h = self.agent(agent_id).ok_or_else(|| format!("unknown agent: {agent_id}"))?;
        h.cmd_tx
            .send(cmd)
            .map_err(|_| format!("agent {agent_id} is not running"))
    }

    pub fn send_prompt(&self, agent_id: &str, text: String) -> Result<(), String> {
        self.send(agent_id, AcpCommand::Prompt(text))
    }

    pub fn new_session(&self, agent_id: &str) -> Result<(), String> {
        self.send(agent_id, AcpCommand::NewSession)
    }

    pub fn resume_session(&self, agent_id: &str, db_id: String) -> Result<(), String> {
        self.send(agent_id, AcpCommand::Resume(db_id))
    }

    pub fn respond_permission(&self, agent_id: &str, request_id: String, choice: String) {
        if let Some(h) = self.agent(agent_id) {
            if let Ok(mut map) = h.pending.lock() {
                if let Some(tx) = map.remove(&request_id) {
                    let _ = tx.send(choice);
                }
            }
        }
    }

    /// Clone of the DB handle for read-only queries from async commands.
    pub fn db_handle(&self) -> Arc<Db> {
        self.db.clone()
    }
}
