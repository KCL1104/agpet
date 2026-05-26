//! Per-instance ACP client: spawn the adapter, run the handshake, map streaming
//! `session/update` to pet states + chat events (tagged with the instance id),
//! accept prompts, route permission requests, persist sessions (keyed by type
//! id), and publish agent config (auth methods / models / modes).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::{
    CancelNotification, ContentBlock, ImageContent, InitializeRequest, McpServer, McpServerHttp,
    ModelId, NewSessionRequest, PromptRequest, ProtocolVersion, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, ResourceLink, SelectedPermissionOutcome,
    SessionId, SessionModeId, SessionNotification, SetSessionModeRequest, SetSessionModelRequest,
    TextContent,
};
use agent_client_protocol::{AcpAgent, Agent, ConnectionTo};
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use super::config::AgentDef;
use super::logging::MessageLog;
use super::{AcpCommand, AcpStatus, PendingPermissions};
use crate::db::Db;

const PET_STATE_EVENT: &str = "pet-state";
const CHAT_EVENT: &str = "chat-event";
const PERMISSION_EVENT: &str = "permission-request";
const SESSION_RESET_EVENT: &str = "session-reset";
const AGENT_CONFIG_EVENT: &str = "agent-config";

const SUMMARY_PROMPT: &str =
    "Summarize what this session accomplished in 2-3 sentences. Reply with only the summary, no preamble.";
const RESUME_PREFIX: &str = "You are continuing a previous session. Here is its summary:\n\n";

static PERM_COUNTER: AtomicU64 = AtomicU64::new(0);

fn emit_state(app: &AppHandle, instance_id: &str, state: &str, detail: Option<String>) {
    tracing::debug!(target: "acp.state", "[{instance_id}] -> {state}{}",
        detail.as_deref().map(|d| format!(" ({d})")).unwrap_or_default());
    let _ = app.emit(PET_STATE_EVENT, json!({ "instance_id": instance_id, "state": state, "detail": detail }));
}

/// Store the agent config and emit it to the frontend (auth methods / models / modes).
fn publish_config(app: &AppHandle, instance_id: &str, cfg: &Arc<Mutex<serde_json::Value>>, mut value: serde_json::Value) {
    if let Ok(mut c) = cfg.lock() {
        *c = value.clone();
    }
    if let Some(o) = value.as_object_mut() {
        o.insert("instance_id".into(), json!(instance_id));
    }
    let _ = app.emit(AGENT_CONFIG_EVENT, value);
}

fn cur_id(m: &Arc<Mutex<Option<String>>>) -> Option<String> {
    m.lock().ok().and_then(|g| g.clone())
}

#[allow(clippy::too_many_arguments)]
pub fn start(
    app: AppHandle,
    instance_id: String,
    type_id: String,
    def: AgentDef,
    cwd: std::path::PathBuf,
    message_log_path: std::path::PathBuf,
    status: Arc<Mutex<AcpStatus>>,
    cmd_rx: mpsc::UnboundedReceiver<AcpCommand>,
    cancel_rx: mpsc::UnboundedReceiver<()>,
    pending: PendingPermissions,
    db: Arc<Db>,
    agent_cfg: Arc<Mutex<serde_json::Value>>,
    busy: Arc<AtomicBool>,
    mcp_url: Option<String>,
) -> Option<tauri::async_runtime::JoinHandle<()>> {
    if std::env::var_os("CLAUDECODE").is_some() {
        std::env::remove_var("CLAUDECODE");
    }

    let message_log = match MessageLog::open(&message_log_path) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("[{instance_id}] cannot open message log: {e}");
            set_status(&status, AcpStatus::Error { message: format!("open message log: {e}") });
            return None;
        }
    };

    let argv = def.argv();
    let agent_name = def.name.clone();
    let workdir = cwd.to_string_lossy().to_string();
    tracing::info!("[{instance_id}] starting adapter: {argv:?} (cwd {})", cwd.display());

    let current_db_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let turn_text: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    // Accumulates agent_thought_chunk text for the current turn (stored once at
    // turn end as a single `thinking` event, mirroring `turn_text`/agent_message).
    let thought_text: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let pending_context: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let handle = tauri::async_runtime::spawn(async move {
        let iid_notif = instance_id.clone();
        let iid_perm = instance_id.clone();
        let iid_main = instance_id.clone();
        let type_main = type_id.clone();

        let log_for_debug = message_log.clone();
        let agent = match AcpAgent::from_args(argv) {
            Ok(a) => a.with_debug(move |line, dir| log_for_debug.record(dir, line)),
            Err(e) => {
                set_status(&status, AcpStatus::Error { message: format!("build agent command: {e}") });
                emit_state(&app, &instance_id, "error", Some(format!("{e}")));
                return;
            }
        };

        set_status(&status, AcpStatus::Connecting);
        emit_state(&app, &instance_id, "connecting", None);

        let status_main = status.clone();
        let cwd_main = cwd.clone();
        let app_notif = app.clone();
        let app_perm = app.clone();
        let app_main = app.clone();
        let pending_perm = pending.clone();
        let db_notif = db.clone();
        let db_main = db.clone();
        let cur_id_notif = current_db_id.clone();
        let cur_id_main = current_db_id.clone();
        let turn_notif = turn_text.clone();
        let turn_main = turn_text.clone();
        let thought_notif = thought_text.clone();
        let thought_main = thought_text.clone();
        let ctx_main = pending_context.clone();
        let cfg_main = agent_cfg.clone();

        let result = agent_client_protocol::Client
            .builder()
            .name("agpet")
            .on_receive_notification(
                async move |notification: SessionNotification, _cx| {
                    if let Some((state, detail)) = map_update(&notification) {
                        emit_state(&app_notif, &iid_notif, state, detail);
                    }
                    if let Some(mut ev) = chat_event(&notification) {
                        if let Some(obj) = ev.as_object_mut() {
                            obj.insert("instance_id".into(), json!(iid_notif));
                        }
                        let _ = app_notif.emit(CHAT_EVENT, ev);
                    }
                    record_update(&notification, &db_notif, &cur_id_notif, &turn_notif, &thought_notif).await;
                    Ok(())
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                async move |request: RequestPermissionRequest, responder, _connection| {
                    let rv = serde_json::to_value(&request).unwrap_or_default();
                    let request_id = rv
                        .get("toolCall")
                        .and_then(|t| t.get("toolCallId"))
                        .and_then(|x| x.as_str())
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("perm-{}", PERM_COUNTER.fetch_add(1, Ordering::Relaxed)));
                    let title = permission_title(&request);
                    let options_json: Vec<serde_json::Value> = request
                        .options
                        .iter()
                        .map(|o| serde_json::to_value(o).unwrap_or(serde_json::Value::Null))
                        .collect();

                    let (tx, rx) = oneshot::channel::<String>();
                    if let Ok(mut m) = pending_perm.lock() {
                        m.insert(request_id.clone(), tx);
                    }
                    emit_state(&app_perm, &iid_perm, "permission", Some(title.clone()));
                    let _ = app_perm.emit(PERMISSION_EVENT, json!({
                        "instance_id": iid_perm, "request_id": request_id, "title": title, "options": options_json,
                    }));

                    let chosen = rx.await.ok();
                    if let Ok(mut m) = pending_perm.lock() {
                        m.remove(&request_id);
                    }
                    let typed = chosen.as_ref().and_then(|opt_id| {
                        request.options.iter().find(|o| {
                            serde_json::to_value(o).ok()
                                .and_then(|v| v.get("optionId").and_then(|x| x.as_str()).map(|s| s == opt_id))
                                .unwrap_or(false)
                        }).map(|o| o.option_id.clone())
                    });
                    match typed {
                        Some(id) => responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id)),
                        )),
                        None => responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        )),
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_with(agent, move |conn: ConnectionTo<Agent>| async move {
                // initialize
                let init = match conn
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task().await
                {
                    Ok(r) => r,
                    Err(e) => {
                        set_status(&status_main, AcpStatus::Error { message: format!("initialize failed: {e}") });
                        emit_state(&app_main, &iid_main, "error", Some("initialize failed".into()));
                        return Ok(());
                    }
                };
                let init_json = serde_json::to_value(&init).unwrap_or_default();
                tracing::info!(
                    "[{iid_main}] agentCapabilities: {}",
                    init_json.get("agentCapabilities").cloned().unwrap_or(serde_json::Value::Null)
                );
                publish_config(&app_main, &iid_main, &cfg_main, json!({
                    "agent_info": init_json.get("agentInfo"),
                    "auth_methods": init_json.get("authMethods"),
                }));

                // session/new — attach agpet's MCP delegate server if the agent
                // supports HTTP MCP (so it gets the `delegate`/`list_agents` tools).
                let mut new_req = NewSessionRequest::new(cwd_main.clone());
                if let Some(url) = &mcp_url {
                    let http_ok = init_json
                        .get("agentCapabilities")
                        .and_then(|c| c.get("mcpCapabilities"))
                        .and_then(|m| m.get("http"))
                        .and_then(|h| h.as_bool())
                        .unwrap_or(false);
                    if http_ok {
                        new_req = new_req.mcp_servers(vec![McpServer::Http(McpServerHttp::new("agpet", url.clone()))]);
                    } else {
                        tracing::info!("[{iid_main}] agent has no http MCP capability; delegate tool unavailable");
                    }
                }
                let sess = match conn
                    .send_request(new_req)
                    .block_task().await
                {
                    Ok(r) => r,
                    Err(e) => {
                        let msg = e.to_string();
                        let (st, state) = if is_auth_required(&msg) {
                            (AcpStatus::AuthRequired { message: format!("{agent_name} needs login.") }, "auth_required")
                        } else {
                            (AcpStatus::Error { message: format!("session/new failed: {msg}") }, "error")
                        };
                        set_status(&status_main, st);
                        emit_state(&app_main, &iid_main, state, None);
                        return Ok(());
                    }
                };
                let sess_json = serde_json::to_value(&sess).unwrap_or_default();
                let mut acp_session: SessionId = sess.session_id;

                publish_config(&app_main, &iid_main, &cfg_main, json!({
                    "agent_info": init_json.get("agentInfo"),
                    "auth_methods": init_json.get("authMethods"),
                    "models": sess_json.get("models"),
                    "modes": sess_json.get("modes"),
                }));

                tracing::info!("[{iid_main}] handshake complete (session {})", acp_session.0);
                set_status(&status_main, AcpStatus::Connected { session_id: acp_session.0.to_string() });
                match db_main.create_session(&type_main, Some(&workdir), None, None).await {
                    Ok(id) => { if let Ok(mut g) = cur_id_main.lock() { *g = Some(id); } }
                    Err(e) => tracing::error!("[{iid_main}] create_session failed: {e}"),
                }
                emit_state(&app_main, &iid_main, "idle", None);

                let mut rx = cmd_rx;
                let mut cancel_rx = cancel_rx;
                while let Some(cmd) = rx.recv().await {
                    match cmd {
                        AcpCommand::Prompt { text, files, images } => {
                            let ctx = ctx_main.lock().ok().and_then(|mut g| g.take());
                            let full_text = match &ctx {
                                Some(c) => format!("{c}\n\n---\n\nUser: {text}"),
                                None => text.clone(),
                            };
                            if let Some(id) = cur_id(&cur_id_main) {
                                let _ = db_main.set_initial_prompt(&id, &text).await;
                                let _ = db_main.append_event(&id, "user_message", &json!({"text": text}).to_string()).await;
                            }
                            if let Ok(mut t) = turn_main.lock() { t.clear(); }
                            if let Ok(mut t) = thought_main.lock() { t.clear(); }
                            // Text block, then a resource link per @-mentioned file so the
                            // agent can read it (the @path also stays in the text as a fallback).
                            let mut blocks = vec![ContentBlock::Text(TextContent::new(full_text))];
                            for rel in &files {
                                let uri = file_uri(&cwd_main.join(rel));
                                blocks.push(ContentBlock::ResourceLink(ResourceLink::new(rel.clone(), uri)));
                            }
                            for img in &images {
                                blocks.push(ContentBlock::Image(ImageContent::new(img.data.clone(), img.mime.clone())));
                            }
                            let req = PromptRequest::new(acp_session.clone(), blocks);
                            // Drop any stale cancel signals (e.g. Stop pressed while idle),
                            // then race the turn against the Stop button.
                            while cancel_rx.try_recv().is_ok() {}
                            busy.store(true, Ordering::SeqCst); // delegate() refuses busy targets
                            let mut prompt_fut = std::pin::pin!(conn.send_request(req).block_task());
                            let result = loop {
                                tokio::select! {
                                    r = &mut prompt_fut => break r,
                                    Some(_) = cancel_rx.recv() => {
                                        let _ = conn.send_notification(CancelNotification::new(acp_session.clone()));
                                        // keep awaiting the now-cancelled turn's response
                                    }
                                }
                            };
                            busy.store(false, Ordering::SeqCst);
                            match result {
                                Ok(_) => {
                                    record_turn(&db_main, &cur_id_main, &turn_main, &thought_main).await;
                                    emit_state(&app_main, &iid_main, "completed", None);
                                }
                                Err(e) => {
                                    tracing::warn!("[{iid_main}] prompt failed: {e}");
                                    emit_state(&app_main, &iid_main, "error", Some("prompt failed".into()));
                                }
                            }
                        }
                        AcpCommand::NewSession => {
                            if let Some(id) = cur_id(&cur_id_main) {
                                summarize_and_finish(&conn, &acp_session, &db_main, &id, &turn_main, &app_main, &iid_main).await;
                            }
                            match open_session(&conn, &cwd_main, &db_main, &type_main, &workdir, None).await {
                                Some((new_acp, new_id)) => {
                                    acp_session = new_acp;
                                    if let Ok(mut g) = cur_id_main.lock() { *g = Some(new_id); }
                                    if let Ok(mut g) = ctx_main.lock() { *g = None; }
                                    let _ = app_main.emit(SESSION_RESET_EVENT, json!({"instance_id": iid_main}));
                                    emit_state(&app_main, &iid_main, "idle", None);
                                }
                                None => emit_state(&app_main, &iid_main, "error", Some("new session failed".into())),
                            }
                        }
                        AcpCommand::SetMode(mode_id) => {
                            let _ = conn
                                .send_request(SetSessionModeRequest::new(acp_session.clone(), SessionModeId::new(mode_id)))
                                .block_task().await;
                        }
                        AcpCommand::SetModel(model_id) => {
                            let _ = conn
                                .send_request(SetSessionModelRequest::new(acp_session.clone(), ModelId::new(model_id)))
                                .block_task().await;
                        }
                        AcpCommand::RunStep { text, reply } => {
                            if let Ok(mut t) = turn_main.lock() { t.clear(); }
                            if let Ok(mut t) = thought_main.lock() { t.clear(); }
                            if let Some(id) = cur_id(&cur_id_main) {
                                let _ = db_main.set_initial_prompt(&id, &text).await;
                                let _ = db_main.append_event(&id, "user_message", &json!({"text": text}).to_string()).await;
                            }
                            let req = PromptRequest::new(acp_session.clone(), vec![ContentBlock::Text(TextContent::new(text))]);
                            busy.store(true, Ordering::SeqCst);
                            let step = conn.send_request(req).block_task().await;
                            busy.store(false, Ordering::SeqCst);
                            match step {
                                Ok(_) => {
                                    let agent_text = record_turn(&db_main, &cur_id_main, &turn_main, &thought_main).await;
                                    emit_state(&app_main, &iid_main, "completed", None);
                                    let _ = reply.send(Ok(agent_text));
                                }
                                Err(e) => {
                                    tracing::warn!("[{iid_main}] workflow step failed: {e}");
                                    emit_state(&app_main, &iid_main, "error", Some("step failed".into()));
                                    let _ = reply.send(Err(e.to_string()));
                                }
                            }
                        }
                        AcpCommand::Resume(prev_id) => {
                            if let Some(id) = cur_id(&cur_id_main) {
                                summarize_and_finish(&conn, &acp_session, &db_main, &id, &turn_main, &app_main, &iid_main).await;
                            }
                            let prev_summary = db_main.get_session(&prev_id).await.ok().flatten().and_then(|r| r.summary);
                            match open_session(&conn, &cwd_main, &db_main, &type_main, &workdir, Some(&prev_id)).await {
                                Some((new_acp, new_id)) => {
                                    acp_session = new_acp;
                                    if let Ok(mut g) = cur_id_main.lock() { *g = Some(new_id); }
                                    if let Ok(mut g) = ctx_main.lock() {
                                        *g = prev_summary.map(|s| format!("{RESUME_PREFIX}{s}"));
                                    }
                                    let _ = app_main.emit(SESSION_RESET_EVENT, json!({"instance_id": iid_main, "resumed_from": prev_id}));
                                    emit_state(&app_main, &iid_main, "idle", None);
                                }
                                None => emit_state(&app_main, &iid_main, "error", Some("resume failed".into())),
                            }
                        }
                    }
                }
                Ok(())
            })
            .await;

        match result {
            Ok(()) => {
                if let Ok(mut g) = status.lock() {
                    if matches!(*g, AcpStatus::Connected { .. } | AcpStatus::Connecting) {
                        *g = AcpStatus::Exited { message: "adapter process ended".into() };
                        emit_state(&app, &instance_id, "exited", None);
                    }
                }
            }
            Err(e) => {
                tracing::error!("[{instance_id}] connection ended: {e}");
                if let Ok(mut g) = status.lock() {
                    if !matches!(*g, AcpStatus::AuthRequired { .. } | AcpStatus::Error { .. }) {
                        *g = AcpStatus::Exited { message: e.to_string() };
                        emit_state(&app, &instance_id, "exited", None);
                    }
                }
            }
        }
    });
    Some(handle)
}

#[allow(clippy::too_many_arguments)]
async fn summarize_and_finish(
    conn: &ConnectionTo<Agent>,
    acp_session: &SessionId,
    db: &Db,
    db_id: &str,
    turn_text: &Arc<Mutex<String>>,
    app: &AppHandle,
    instance_id: &str,
) {
    if let Ok(mut t) = turn_text.lock() { t.clear(); }
    emit_state(app, instance_id, "thinking", Some("summarizing".into()));
    let summary = match conn
        .send_request(PromptRequest::new(
            acp_session.clone(),
            vec![ContentBlock::Text(TextContent::new(SUMMARY_PROMPT.to_string()))],
        ))
        .block_task().await
    {
        Ok(_) => {
            let s = turn_text.lock().map(|t| t.clone()).unwrap_or_default();
            (!s.trim().is_empty()).then_some(s)
        }
        Err(e) => {
            tracing::warn!("[{instance_id}] summary prompt failed: {e}");
            None
        }
    };
    if let Err(e) = db.finish_session(db_id, summary.as_deref(), "completed").await {
        tracing::warn!("[{instance_id}] finish_session failed: {e}");
    }
}

async fn open_session(
    conn: &ConnectionTo<Agent>,
    cwd: &std::path::Path,
    db: &Db,
    type_id: &str,
    workdir: &str,
    parent: Option<&str>,
) -> Option<(SessionId, String)> {
    let acp = match conn
        .send_request(NewSessionRequest::new(cwd.to_path_buf()))
        .block_task().await
    {
        Ok(r) => r.session_id,
        Err(e) => {
            tracing::warn!("session/new failed: {e}");
            return None;
        }
    };
    let db_id = db.create_session(type_id, Some(workdir), None, parent).await.ok()?;
    Some((acp, db_id))
}

/// Persist a finished turn: the accumulated thinking (if any), then the agent
/// message (if any). Thinking is written first so its DB timestamp precedes the
/// reply. Returns the agent message text (used as a workflow step's output).
async fn record_turn(
    db: &Db,
    cur_id_state: &Arc<Mutex<Option<String>>>,
    turn_text: &Arc<Mutex<String>>,
    thought_text: &Arc<Mutex<String>>,
) -> String {
    let thoughts = thought_text.lock().map(|t| t.clone()).unwrap_or_default();
    let agent_text = turn_text.lock().map(|t| t.clone()).unwrap_or_default();
    if let Some(id) = cur_id(cur_id_state) {
        if !thoughts.is_empty() {
            let _ = db.append_event(&id, "thinking", &json!({ "text": thoughts }).to_string()).await;
        }
        if !agent_text.is_empty() {
            let _ = db.append_event(&id, "agent_message", &json!({ "text": agent_text }).to_string()).await;
        }
    }
    agent_text
}

async fn record_update(
    notification: &SessionNotification,
    db: &Db,
    cur_id_state: &Arc<Mutex<Option<String>>>,
    turn_text: &Arc<Mutex<String>>,
    thought_text: &Arc<Mutex<String>>,
) {
    let Ok(v) = serde_json::to_value(notification) else { return };
    let Some(update) = v.get("update") else { return };
    let Some(kind) = update.get("sessionUpdate").and_then(|k| k.as_str()) else { return };

    if kind == "agent_message_chunk" {
        if let Some(text) = update.get("content").and_then(extract_text) {
            if let Ok(mut t) = turn_text.lock() {
                t.push_str(&text);
            }
        }
    }
    if kind == "agent_thought_chunk" {
        if let Some(text) = update.get("content").and_then(extract_text) {
            if let Ok(mut t) = thought_text.lock() {
                t.push_str(&text);
            }
        }
    }

    let Some(session_id) = cur_id(cur_id_state) else { return };
    if kind == "tool_call" {
        let _ = db.append_event(&session_id, "tool_call", &update.to_string()).await;
        if let Some(fp) = update.get("rawInput").and_then(|r| r.get("file_path")).and_then(|x| x.as_str()) {
            let tool = update
                .get("_meta").and_then(|m| m.get("claudeCode")).and_then(|c| c.get("toolName"))
                .and_then(|x| x.as_str()).unwrap_or("");
            let op = match tool {
                "Write" => "create",
                "Edit" | "MultiEdit" | "NotebookEdit" => "edit",
                "Read" => "read",
                _ => "other",
            };
            let _ = db.append_file(&session_id, fp, op).await;
        }
    }
}

fn map_update(notification: &SessionNotification) -> Option<(&'static str, Option<String>)> {
    let v = serde_json::to_value(notification).ok()?;
    let update = v.get("update")?;
    let kind = update.get("sessionUpdate")?.as_str()?;
    let title = || update.get("title").and_then(|t| t.as_str()).map(str::to_string);
    match kind {
        "agent_thought_chunk" => Some(("thinking", None)),
        "agent_message_chunk" => Some(("responding", None)),
        "plan" => Some(("thinking", Some("planning".into()))),
        "tool_call" => Some(("tool_running", title())),
        "tool_call_update" => match update.get("status").and_then(|s| s.as_str()) {
            Some("pending") | Some("in_progress") => Some(("tool_running", title())),
            _ => None,
        },
        _ => None,
    }
}

fn chat_event(notification: &SessionNotification) -> Option<serde_json::Value> {
    let v = serde_json::to_value(notification).ok()?;
    let update = v.get("update")?;
    let kind = update.get("sessionUpdate")?.as_str()?;
    match kind {
        "agent_message_chunk" => Some(json!({ "kind": "agent_message", "text": update.get("content").and_then(extract_text).unwrap_or_default() })),
        "agent_thought_chunk" => Some(json!({ "kind": "agent_thought", "text": update.get("content").and_then(extract_text).unwrap_or_default() })),
        "tool_call" => Some(json!({
            "kind": "tool_call",
            "tool_call_id": update.get("toolCallId"),
            "title": update.get("title"),
            "status": update.get("status"),
            "tool_kind": update.get("kind"),     // read/edit/execute/search/…
            "locations": tool_locations(update), // files this tool touches
        })),
        "tool_call_update" => Some(json!({
            "kind": "tool_update",
            "tool_call_id": update.get("toolCallId"),
            "status": update.get("status"),
            "tool_kind": update.get("kind"),
            // The tool's result text (file contents, command output, …), if present.
            "result": update.get("content").and_then(extract_text),
            "diff": tool_diff(update),           // file edit shown as old → new
            "locations": tool_locations(update),
        })),
        "plan" => Some(json!({ "kind": "plan", "entries": update.get("entries") })),
        "available_commands_update" => Some(json!({
            "kind": "commands",
            "commands": update.get("availableCommands"),
        })),
        "current_mode_update" => Some(json!({ "kind": "mode", "mode_id": update.get("currentModeId") })),
        _ => None,
    }
}

/// File paths a tool call touches (for "follow-along" display).
fn tool_locations(update: &serde_json::Value) -> Vec<String> {
    update
        .get("locations")
        .and_then(|l| l.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.get("path").and_then(|p| p.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// First file-edit diff in a tool call's content, as `{path, old, new}`.
fn tool_diff(update: &serde_json::Value) -> Option<serde_json::Value> {
    let arr = update.get("content")?.as_array()?;
    arr.iter()
        .find(|it| it.get("type").and_then(|t| t.as_str()) == Some("diff"))
        .map(|d| json!({ "path": d.get("path"), "old": d.get("oldText"), "new": d.get("newText") }))
}

/// `file://` URI for an absolute path (forward slashes; works for both
/// `/unix/abs` and `C:\windows\abs` inputs).
fn file_uri(path: &std::path::Path) -> String {
    let fwd = path.to_string_lossy().replace('\\', "/");
    if fwd.starts_with('/') {
        format!("file://{fwd}")
    } else {
        format!("file:///{fwd}")
    }
}

fn extract_text(v: &serde_json::Value) -> Option<String> {
    if let Some(t) = v.get("text").and_then(|x| x.as_str()) {
        return Some(t.to_string());
    }
    if let Some(inner) = v.get("content") {
        if let Some(t) = extract_text(inner) {
            return Some(t);
        }
    }
    if let Some(arr) = v.as_array() {
        let mut s = String::new();
        for it in arr {
            if let Some(t) = extract_text(it) {
                s.push_str(&t);
            }
        }
        if !s.is_empty() {
            return Some(s);
        }
    }
    None
}

fn permission_title(request: &RequestPermissionRequest) -> String {
    serde_json::to_value(request)
        .ok()
        .and_then(|v| {
            v.get("toolCall").and_then(|t| t.get("title")).and_then(|t| t.as_str()).map(str::to_string)
        })
        .unwrap_or_else(|| "Permission requested".into())
}

fn set_status(status: &Arc<Mutex<AcpStatus>>, new: AcpStatus) {
    if let Ok(mut g) = status.lock() {
        *g = new;
    }
}

fn is_auth_required(message: &str) -> bool {
    let m = message.to_lowercase();
    m.contains("login") || m.contains("auth") || m.contains("api key") || m.contains("unauthorized")
}
