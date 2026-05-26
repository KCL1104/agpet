//! ACP client core (Milestone 1–3 + UX batch).
//!
//! Manages **agent types** (from `agents.toml`) and dynamically-launched
//! **instances** (pets). The same type can have multiple live instances. Each
//! instance has its own connection / status / command channel / pending-perm
//! map; the DB is shared and keyed by the type id (so history groups per type).

mod client;
pub mod config;
pub mod logging;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};

pub use config::{AgentDef, AgentsConfig};

use crate::db::Db;

/// Commands sent into a live instance's connection.
pub enum AcpCommand {
    Prompt(String),
    NewSession,
    Resume(String),
    SetMode(String),
    SetModel(String),
}

pub type PendingPermissions = Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>;

/// State of one instance's connection.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AcpStatus {
    Connecting,
    Connected { session_id: String },
    AuthRequired { message: String },
    Error { message: String },
    Exited { message: String },
}

/// An agent type (template) for the tray's "New" menu.
#[derive(Debug, Clone, Serialize)]
pub struct TypeInfo {
    pub type_id: String,
    pub name: String,
    pub color: String,
}

/// A running instance, for the frontend to render a pet.
#[derive(Debug, Clone, Serialize)]
pub struct InstanceInfo {
    pub instance_id: String,
    pub type_id: String,
    pub name: String,
    pub color: String,
}

struct Instance {
    instance_id: String,
    type_id: String,
    name: String,
    color: String,
    /// Working directory this instance's sessions run in.
    cwd: PathBuf,
    status: Arc<Mutex<AcpStatus>>,
    cmd_tx: Option<mpsc::UnboundedSender<AcpCommand>>,
    pending: PendingPermissions,
    /// Agent config (authMethods / models / modes) filled in by the client.
    config: Arc<Mutex<serde_json::Value>>,
}

pub struct AcpManager {
    instances: Mutex<Vec<Instance>>,
    next_n: Mutex<HashMap<String, usize>>,
    defs: Vec<AgentDef>,
    db: Arc<Db>,
    app: AppHandle,
    cwd: PathBuf,
    log_dir: PathBuf,
}

impl AcpManager {
    pub fn new(db: Arc<Db>, config: &AgentsConfig, app: AppHandle) -> Self {
        Self {
            instances: Mutex::new(Vec::new()),
            next_n: Mutex::new(HashMap::new()),
            defs: config.agents.clone(),
            db,
            app,
            cwd: config.cwd.clone(),
            log_dir: config.log_dir.clone(),
        }
    }

    /// Launch one instance per declared type (kept for a future startup set).
    #[allow(dead_code)]
    pub fn launch_defaults(&self) {
        let ids: Vec<String> = self.defs.iter().map(|d| d.id.clone()).collect();
        for id in ids {
            let _ = self.launch(&id, None);
        }
    }

    /// Launch a new instance of `type_id` in `cwd` (or the default). Returns its id.
    pub fn launch(&self, type_id: &str, cwd: Option<String>) -> Result<String, String> {
        let def = self
            .defs
            .iter()
            .find(|d| d.id == type_id)
            .ok_or_else(|| format!("unknown agent type: {type_id}"))?
            .clone();

        let cwd_path = cwd
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.cwd.clone());

        let n = {
            let mut c = self.next_n.lock().unwrap();
            let e = c.entry(type_id.to_string()).or_insert(0);
            *e += 1;
            *e
        };
        let instance_id = format!("{type_id}-{n}");
        let name = if n == 1 { def.name.clone() } else { format!("{} {}", def.name, n) };
        let color = def.color.clone();

        let status = Arc::new(Mutex::new(AcpStatus::Connecting));
        let pending: PendingPermissions = Arc::new(Mutex::new(HashMap::new()));
        let cfg = Arc::new(Mutex::new(serde_json::Value::Null));
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        self.instances.lock().unwrap().push(Instance {
            instance_id: instance_id.clone(),
            type_id: type_id.to_string(),
            name: name.clone(),
            color: color.clone(),
            cwd: cwd_path.clone(),
            status: status.clone(),
            cmd_tx: Some(cmd_tx),
            pending: pending.clone(),
            config: cfg.clone(),
        });

        let log_path = self.log_dir.join(format!("acp-messages-{instance_id}.jsonl"));
        client::start(
            self.app.clone(),
            instance_id.clone(),
            type_id.to_string(),
            def,
            cwd_path,
            log_path,
            status,
            cmd_rx,
            pending,
            self.db.clone(),
            cfg,
        );

        let _ = self.app.emit("instance-added", json!({
            "instance_id": instance_id, "type_id": type_id, "name": name, "color": color
        }));
        Ok(instance_id)
    }

    /// Close an instance: drop its command channel (stops the adapter) and remove it.
    pub fn close(&self, instance_id: &str) -> Result<(), String> {
        {
            let mut insts = self.instances.lock().unwrap();
            let pos = insts
                .iter()
                .position(|i| i.instance_id == instance_id)
                .ok_or_else(|| format!("unknown instance: {instance_id}"))?;
            insts.remove(pos); // drops cmd_tx -> client loop ends -> adapter killed
        }
        let _ = self.app.emit("instance-removed", json!({ "instance_id": instance_id }));
        Ok(())
    }

    /// Restart a (stopped/errored) instance's connection, keeping its id.
    pub fn retry(&self, instance_id: &str) -> Result<(), String> {
        let (type_id, cwd_path, status, pending, cfg, cmd_rx) = {
            let mut insts = self.instances.lock().unwrap();
            let inst = insts
                .iter_mut()
                .find(|i| i.instance_id == instance_id)
                .ok_or_else(|| format!("unknown instance: {instance_id}"))?;
            let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
            inst.cmd_tx = Some(cmd_tx); // dropping the old sender stops the old loop
            if let Ok(mut s) = inst.status.lock() {
                *s = AcpStatus::Connecting;
            }
            (
                inst.type_id.clone(),
                inst.cwd.clone(),
                inst.status.clone(),
                inst.pending.clone(),
                inst.config.clone(),
                cmd_rx,
            )
        };
        let def = self
            .defs
            .iter()
            .find(|d| d.id == type_id)
            .ok_or_else(|| format!("type gone: {type_id}"))?
            .clone();
        let log_path = self.log_dir.join(format!("acp-messages-{instance_id}.jsonl"));
        client::start(
            self.app.clone(),
            instance_id.to_string(),
            type_id,
            def,
            cwd_path,
            log_path,
            status,
            cmd_rx,
            pending,
            self.db.clone(),
            cfg,
        );
        Ok(())
    }

    pub fn list_instances(&self) -> Vec<InstanceInfo> {
        self.instances
            .lock()
            .map(|insts| {
                insts
                    .iter()
                    .map(|i| InstanceInfo {
                        instance_id: i.instance_id.clone(),
                        type_id: i.type_id.clone(),
                        name: i.name.clone(),
                        color: i.color.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn list_types(&self) -> Vec<TypeInfo> {
        self.defs
            .iter()
            .map(|d| TypeInfo { type_id: d.id.clone(), name: d.name.clone(), color: d.color.clone() })
            .collect()
    }

    fn sender(&self, instance_id: &str) -> Option<mpsc::UnboundedSender<AcpCommand>> {
        self.instances
            .lock()
            .ok()?
            .iter()
            .find(|i| i.instance_id == instance_id)
            .and_then(|i| i.cmd_tx.clone())
    }

    fn send(&self, instance_id: &str, cmd: AcpCommand) -> Result<(), String> {
        self.sender(instance_id)
            .ok_or_else(|| format!("instance {instance_id} is not running"))?
            .send(cmd)
            .map_err(|_| format!("instance {instance_id} is not running"))
    }

    pub fn send_prompt(&self, instance_id: &str, text: String) -> Result<(), String> {
        self.send(instance_id, AcpCommand::Prompt(text))
    }
    pub fn new_session(&self, instance_id: &str) -> Result<(), String> {
        self.send(instance_id, AcpCommand::NewSession)
    }
    pub fn resume_session(&self, instance_id: &str, db_id: String) -> Result<(), String> {
        self.send(instance_id, AcpCommand::Resume(db_id))
    }
    pub fn set_mode(&self, instance_id: &str, mode_id: String) -> Result<(), String> {
        self.send(instance_id, AcpCommand::SetMode(mode_id))
    }
    pub fn set_model(&self, instance_id: &str, model_id: String) -> Result<(), String> {
        self.send(instance_id, AcpCommand::SetModel(model_id))
    }

    pub fn respond_permission(&self, instance_id: &str, request_id: String, choice: String) {
        let pending = self
            .instances
            .lock()
            .ok()
            .and_then(|insts| insts.iter().find(|i| i.instance_id == instance_id).map(|i| i.pending.clone()));
        if let Some(p) = pending {
            if let Ok(mut map) = p.lock() {
                if let Some(tx) = map.remove(&request_id) {
                    let _ = tx.send(choice);
                }
            }
        }
    }

    pub fn agent_config(&self, instance_id: &str) -> serde_json::Value {
        self.instances
            .lock()
            .ok()
            .and_then(|insts| insts.iter().find(|i| i.instance_id == instance_id).map(|i| i.config.lock().map(|c| c.clone()).unwrap_or(serde_json::Value::Null)))
            .unwrap_or(serde_json::Value::Null)
    }

    pub fn type_of(&self, instance_id: &str) -> Option<String> {
        self.instances
            .lock()
            .ok()?
            .iter()
            .find(|i| i.instance_id == instance_id)
            .map(|i| i.type_id.clone())
    }

    pub fn db_handle(&self) -> Arc<Db> {
        self.db.clone()
    }
}
