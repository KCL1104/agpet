//! ACP client core (Milestone 1–3 + UX batch).
//!
//! Manages **agent types** (from `agents.toml`) and dynamically-launched
//! **instances** (pets). The same type can have multiple live instances. Each
//! instance has its own connection / status / command channel / pending-perm
//! map; the DB is shared and keyed by the type id (so history groups per type).

mod client;
pub mod config;
pub mod logging;
mod workflow;

pub use workflow::Workflow;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};

pub use config::{AgentDef, AgentsConfig};

use crate::db::Db;

/// A pasted image to attach to a prompt (base64 data + MIME type).
#[derive(Clone, serde::Deserialize)]
pub struct PromptImage {
    pub mime: String,
    pub data: String,
}

/// Commands sent into a live instance's connection.
pub enum AcpCommand {
    /// User prompt text plus any @-mentioned files (cwd-relative paths) to
    /// attach as ACP resource links, and any pasted images.
    Prompt { text: String, files: Vec<String>, images: Vec<PromptImage> },
    NewSession,
    Resume(String),
    SetMode(String),
    SetModel(String),
    /// Workflow step: send a prompt and return the agent's full reply text.
    RunStep {
        text: String,
        reply: oneshot::Sender<Result<String, String>>,
    },
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
    /// Out-of-band signal to cancel the in-flight prompt (the main cmd loop is
    /// blocked awaiting the turn, so cancel rides its own channel).
    cancel_tx: Option<mpsc::UnboundedSender<()>>,
    pending: PendingPermissions,
    /// Agent config (authMethods / models / modes) filled in by the client.
    config: Arc<Mutex<serde_json::Value>>,
    /// The connection task; aborted on reload/close to kill a hung adapter.
    task: Option<tauri::async_runtime::JoinHandle<()>>,
    /// True while this instance is mid-turn — `delegate` refuses busy targets to
    /// avoid deadlocking (e.g. the orchestrating "mother", or a delegation cycle).
    busy: Arc<AtomicBool>,
}

pub struct AcpManager {
    instances: Arc<Mutex<Vec<Instance>>>,
    next_n: Mutex<HashMap<String, usize>>,
    defs: Vec<AgentDef>,
    db: Arc<Db>,
    app: AppHandle,
    cwd: PathBuf,
    log_dir: PathBuf,
    /// True while a workflow run is in flight; serializes runs so two workflows
    /// can't interleave prompts on the same agent instance.
    workflow_running: Arc<AtomicBool>,
    /// agpet's MCP server URL (delegate tool); attached to sessions that support
    /// HTTP MCP. None if the server failed to start.
    mcp_url: Option<String>,
    /// Pending non-blocking delegations: handle -> (worker instance to auto-close
    /// once collected, if it was a spawned worktree worker; result receiver).
    delegations: Mutex<HashMap<String, (Option<String>, oneshot::Receiver<Result<String, String>>)>>,
    delegation_seq: AtomicU64,
}

impl AcpManager {
    pub fn new(db: Arc<Db>, config: &AgentsConfig, app: AppHandle, mcp_url: Option<String>) -> Self {
        Self {
            instances: Arc::new(Mutex::new(Vec::new())),
            next_n: Mutex::new(HashMap::new()),
            defs: config.agents.clone(),
            db,
            app,
            cwd: config.cwd.clone(),
            log_dir: config.log_dir.clone(),
            workflow_running: Arc::new(AtomicBool::new(false)),
            mcp_url,
            delegations: Mutex::new(HashMap::new()),
            delegation_seq: AtomicU64::new(1),
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
        let busy = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (cancel_tx, cancel_rx) = mpsc::unbounded_channel();

        self.instances.lock().unwrap().push(Instance {
            instance_id: instance_id.clone(),
            type_id: type_id.to_string(),
            name: name.clone(),
            color: color.clone(),
            cwd: cwd_path.clone(),
            status: status.clone(),
            cmd_tx: Some(cmd_tx),
            cancel_tx: Some(cancel_tx),
            pending: pending.clone(),
            config: cfg.clone(),
            task: None,
            busy: busy.clone(),
        });

        let log_path = self.log_dir.join(format!("acp-messages-{instance_id}.jsonl"));
        let task = client::start(
            self.app.clone(),
            instance_id.clone(),
            type_id.to_string(),
            def,
            cwd_path,
            log_path,
            status,
            cmd_rx,
            cancel_rx,
            pending,
            self.db.clone(),
            cfg,
            busy,
            self.mcp_url.clone(),
        );
        if let Ok(mut insts) = self.instances.lock() {
            if let Some(inst) = insts.iter_mut().find(|i| i.instance_id == instance_id) {
                inst.task = task;
            }
        }

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
            let inst = insts.remove(pos); // drops cmd_tx -> client loop ends -> adapter killed
            if let Some(h) = inst.task {
                h.abort(); // also kill a task stuck mid-handshake (won't see the dropped cmd_tx)
            }
        }
        let _ = self.app.emit("instance-removed", json!({ "instance_id": instance_id }));
        Ok(())
    }

    /// Restart a (stopped/errored) instance's connection, keeping its id.
    pub fn retry(&self, instance_id: &str) -> Result<(), String> {
        let (type_id, cwd_path, status, pending, cfg, busy, cmd_rx, cancel_rx) = {
            let mut insts = self.instances.lock().unwrap();
            let inst = insts
                .iter_mut()
                .find(|i| i.instance_id == instance_id)
                .ok_or_else(|| format!("unknown instance: {instance_id}"))?;
            let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
            let (cancel_tx, cancel_rx) = mpsc::unbounded_channel();
            inst.cmd_tx = Some(cmd_tx); // dropping the old sender stops the old loop
            inst.cancel_tx = Some(cancel_tx);
            inst.busy.store(false, Ordering::SeqCst);
            if let Some(h) = inst.task.take() {
                h.abort(); // kill the old task — handles the stuck-on-connecting case
            }
            if let Ok(mut s) = inst.status.lock() {
                *s = AcpStatus::Connecting;
            }
            (
                inst.type_id.clone(),
                inst.cwd.clone(),
                inst.status.clone(),
                inst.pending.clone(),
                inst.config.clone(),
                inst.busy.clone(),
                cmd_rx,
                cancel_rx,
            )
        };
        let def = self
            .defs
            .iter()
            .find(|d| d.id == type_id)
            .ok_or_else(|| format!("type gone: {type_id}"))?
            .clone();
        let log_path = self.log_dir.join(format!("acp-messages-{instance_id}.jsonl"));
        let task = client::start(
            self.app.clone(),
            instance_id.to_string(),
            type_id,
            def,
            cwd_path,
            log_path,
            status,
            cmd_rx,
            cancel_rx,
            pending,
            self.db.clone(),
            cfg,
            busy,
            self.mcp_url.clone(),
        );
        if let Ok(mut insts) = self.instances.lock() {
            if let Some(inst) = insts.iter_mut().find(|i| i.instance_id == instance_id) {
                inst.task = task;
            }
        }
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

    pub fn send_prompt(&self, instance_id: &str, text: String, files: Vec<String>, images: Vec<PromptImage>) -> Result<(), String> {
        self.send(instance_id, AcpCommand::Prompt { text, files, images })
    }

    /// Cancel the in-flight prompt turn on an instance (out-of-band signal).
    pub fn cancel(&self, instance_id: &str) -> Result<(), String> {
        let tx = self
            .instances
            .lock()
            .ok()
            .and_then(|insts| insts.iter().find(|i| i.instance_id == instance_id).and_then(|i| i.cancel_tx.clone()))
            .ok_or_else(|| format!("instance {instance_id} is not running"))?;
        tx.send(()).map_err(|_| format!("instance {instance_id} is not running"))
    }

    /// Rename an instance (display name; also how `delegate` resolves a target).
    pub fn rename(&self, instance_id: &str, name: String) -> Result<(), String> {
        let mut insts = self.instances.lock().map_err(|_| "lock poisoned".to_string())?;
        let inst = insts
            .iter_mut()
            .find(|i| i.instance_id == instance_id)
            .ok_or_else(|| format!("unknown instance: {instance_id}"))?;
        inst.name = name;
        Ok(())
    }

    /// Delegate a task to another running agent (by name, case-insensitive, or
    /// id). With `wait`, returns its turn output; else dispatches and returns at
    /// once. Refuses **busy** targets so a blocked orchestrator / delegation
    /// cycle can't deadlock.
    pub async fn delegate(&self, agent: &str, task: String, wait: bool) -> Result<String, String> {
        // Resolve the named target → its id + type + repo + (fallback) sender.
        let (target_id, target_type, target_cwd, target_name, existing_tx, existing_busy) = {
            let insts = self.instances.lock().map_err(|_| "lock poisoned".to_string())?;
            let inst = insts
                .iter()
                .find(|i| i.instance_id == agent || i.name.eq_ignore_ascii_case(agent))
                .ok_or_else(|| format!("no running agent named '{agent}'"))?;
            (
                inst.instance_id.clone(),
                inst.type_id.clone(),
                inst.cwd.clone(),
                inst.name.clone(),
                inst.cmd_tx.clone(),
                inst.busy.load(Ordering::SeqCst),
            )
        };
        let n = self.delegation_seq.fetch_add(1, Ordering::SeqCst);

        // Prefer running the delegated work in an isolated git worktree: spawn a
        // fresh worker of the target's type into a new branch off its repo (the
        // named agent itself stays untouched). Fall back to routing to the
        // existing agent when it isn't in a git repo. `spawned` is the worker to
        // auto-close once its result is collected (its worktree is kept).
        let (tx, where_note, spawned) = if crate::git::is_repo(&target_cwd) {
            let branch = format!("agpet/delegate/{target_type}-{n}");
            let path = crate::git::worktree_create(&target_cwd, &branch)?;
            let worker_id = self.launch(&target_type, Some(path.to_string_lossy().into_owned()))?;
            let tx = self
                .sender(&worker_id)
                .ok_or_else(|| format!("worker {worker_id} failed to start"))?;
            // 📦 fly the task from the named agent to its new worktree worker.
            let _ = self.app.emit("workflow-handoff", json!({ "from_instance": target_id, "to_instance": worker_id }));
            (tx, format!(" (worker in worktree {branch})"), Some(worker_id))
        } else {
            if existing_busy {
                return Err(format!("agent '{target_name}' is busy — try again once it's idle"));
            }
            let tx = existing_tx.ok_or_else(|| format!("agent '{target_name}' is not running"))?;
            (tx, String::new(), None)
        };

        let (rtx, rrx) = oneshot::channel::<Result<String, String>>();
        tx.send(AcpCommand::RunStep { text: task, reply: rtx })
            .map_err(|_| format!("agent '{target_name}' stopped"))?;
        if !wait {
            // Stash the result receiver (+ worker to auto-close) under a handle so
            // `collect` can fetch it after the caller has done its own work.
            let handle = format!("dlg-{n}");
            if let Ok(mut m) = self.delegations.lock() {
                m.insert(handle.clone(), (spawned, rrx));
            }
            return Ok(format!(
                "dispatched{where_note} in the background (handle: {handle}). Do your own work, then call `collect` with handle \"{handle}\" to get its result."
            ));
        }
        let result = match rrx.await {
            Ok(Ok(out)) => Ok(if where_note.is_empty() { out } else { format!("{out}{where_note}") }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(format!("agent '{target_name}' did not reply")),
        };
        if let Some(w) = spawned {
            let _ = self.close(&w); // auto-close the worker pet (worktree is kept)
        }
        result
    }

    /// Await and return the result of a delegation previously dispatched with
    /// `wait=false`, by its handle. Blocks until that sub-agent finishes (or
    /// returns immediately if it already has).
    pub async fn collect(&self, handle: &str) -> Result<String, String> {
        let (worker, rrx) = {
            let mut m = self.delegations.lock().map_err(|_| "lock poisoned".to_string())?;
            m.remove(handle).ok_or_else(|| format!("no pending delegation with handle '{handle}'"))?
        };
        let result = match rrx.await {
            Ok(r) => r,
            Err(_) => Err(format!("delegation '{handle}' produced no result")),
        };
        if let Some(w) = worker {
            let _ = self.close(&w); // auto-close the worker pet (worktree is kept)
        }
        result
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

    pub fn cwd_of(&self, instance_id: &str) -> Option<PathBuf> {
        self.instances
            .lock()
            .ok()?
            .iter()
            .find(|i| i.instance_id == instance_id)
            .map(|i| i.cwd.clone())
    }

    /// Files under an instance's working dir (cwd-relative, `/`-separated), for
    /// the chat `@`-mention picker. Skips heavy/noise dirs and caps the count.
    pub fn list_dir_files(&self, instance_id: &str) -> Result<Vec<String>, String> {
        let root = self
            .cwd_of(instance_id)
            .ok_or_else(|| format!("instance {instance_id} not found"))?;
        let mut out = Vec::new();
        walk_dir(&root, &root, &mut out, 0);
        out.sort();
        Ok(out)
    }

    pub fn db_handle(&self) -> Arc<Db> {
        self.db.clone()
    }

    pub fn list_workflows(&self) -> Vec<Workflow> {
        workflow::load_all(&self.app)
    }

    /// Start a workflow run (spawns a router task that drives the steps). Rejects
    /// a second run while one is in flight so two workflows can't interleave
    /// prompts on the same instance.
    pub fn run_workflow(&self, workflow_id: &str, user_input: String) -> Result<(), String> {
        // Resolve first so an unknown-id error doesn't leave the flag set.
        let wf = workflow::load_all(&self.app)
            .into_iter()
            .find(|w| w.id == workflow_id)
            .ok_or_else(|| format!("unknown workflow: {workflow_id}"))?;
        if self
            .workflow_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("A workflow is already running — wait for it to finish.".into());
        }
        workflow::run(
            self.app.clone(),
            self.instances.clone(),
            wf,
            user_input,
            self.workflow_running.clone(),
        );
        Ok(())
    }

    /// Ad-hoc sequential handoff over explicit worker ids (vertical worktree task).
    pub fn run_handoff(&self, worker_ids: Vec<String>, text: String) -> Result<(), String> {
        if self
            .workflow_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("A workflow is already running — wait for it to finish.".into());
        }
        workflow::run_handoff(
            self.app.clone(),
            self.instances.clone(),
            worker_ids,
            text,
            self.workflow_running.clone(),
        );
        Ok(())
    }
}

/// Dirs never descended into when listing files for the `@`-mention picker.
const SKIP_DIRS: &[&str] = &[
    ".git", "node_modules", "target", "dist", "build", ".next", ".nuxt",
    ".svelte-kit", ".venv", "venv", "__pycache__", ".cache", "vendor",
    ".gradle", ".idea", ".vscode",
];
const MAX_LISTED_FILES: usize = 3000;

/// Recursively collect files under `root` as `/`-separated paths relative to
/// `root`. Bounded by [`MAX_LISTED_FILES`] and a depth limit.
fn walk_dir(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>, depth: usize) {
    if out.len() >= MAX_LISTED_FILES || depth > 12 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if out.len() >= MAX_LISTED_FILES {
            return;
        }
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if ft.is_dir() {
            if SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            walk_dir(root, &path, out, depth + 1);
        } else if ft.is_file() {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}
