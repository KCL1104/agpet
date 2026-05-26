//! Per-agent ACP client: spawn the adapter, run the handshake, map streaming
//! `session/update` to pet states + chat events (tagged with the agent id),
//! accept prompts, route permission requests, and persist sessions to SQLite.
//! One instance runs per declared agent.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionId, SessionNotification, TextContent,
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

const SUMMARY_PROMPT: &str =
    "Summarize what this session accomplished in 2-3 sentences. Reply with only the summary, no preamble.";
const RESUME_PREFIX: &str = "You are continuing a previous session. Here is its summary:\n\n";

static PERM_COUNTER: AtomicU64 = AtomicU64::new(0);

fn emit_state(app: &AppHandle, agent_id: &str, state: &str, detail: Option<String>) {
    tracing::debug!(target: "acp.state", "[{agent_id}] -> {state}{}",
        detail.as_deref().map(|d| format!(" ({d})")).unwrap_or_default());
    let _ = app.emit(PET_STATE_EVENT, json!({ "agent_id": agent_id, "state": state, "detail": detail }));
}

fn cur_id(m: &Arc<Mutex<Option<String>>>) -> Option<String> {
    m.lock().ok().and_then(|g| g.clone())
}

/// Spawn one agent's adapter and drive its handshake + command loop.
#[allow(clippy::too_many_arguments)]
pub fn start(
    app: AppHandle,
    def: AgentDef,
    cwd: std::path::PathBuf,
    message_log_path: std::path::PathBuf,
    status: Arc<Mutex<AcpStatus>>,
    cmd_rx: mpsc::UnboundedReceiver<AcpCommand>,
    pending: PendingPermissions,
    db: Arc<Db>,
) {
    // Claude Code refuses to nest; clear the guard for spawned adapters.
    if std::env::var_os("CLAUDECODE").is_some() {
        std::env::remove_var("CLAUDECODE");
    }

    let agent_id = def.id.clone();
    let message_log = match MessageLog::open(&message_log_path) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("[{agent_id}] cannot open message log: {e}");
            set_status(&status, AcpStatus::Error { message: format!("open message log: {e}") });
            return;
        }
    };

    let argv = def.argv();
    let workdir = cwd.to_string_lossy().to_string();
    tracing::info!("[{agent_id}] starting adapter: {argv:?} (cwd {})", cwd.display());

    let current_db_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let turn_text: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let pending_context: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    tauri::async_runtime::spawn(async move {
        let aid_notif = agent_id.clone();
        let aid_perm = agent_id.clone();
        let aid_main = agent_id.clone();

        let log_for_debug = message_log.clone();
        let agent = match AcpAgent::from_args(argv) {
            Ok(a) => a.with_debug(move |line, dir| log_for_debug.record(dir, line)),
            Err(e) => {
                set_status(&status, AcpStatus::Error { message: format!("build agent command: {e}") });
                emit_state(&app, &agent_id, "error", Some(format!("{e}")));
                return;
            }
        };

        set_status(&status, AcpStatus::Connecting);
        emit_state(&app, &agent_id, "connecting", None);

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
        let ctx_main = pending_context.clone();

        let result = agent_client_protocol::Client
            .builder()
            .name("agpet")
            .on_receive_notification(
                async move |notification: SessionNotification, _cx| {
                    if let Some((state, detail)) = map_update(&notification) {
                        emit_state(&app_notif, &aid_notif, state, detail);
                    }
                    if let Some(mut ev) = chat_event(&notification) {
                        if let Some(obj) = ev.as_object_mut() {
                            obj.insert("agent_id".into(), json!(aid_notif));
                        }
                        let _ = app_notif.emit(CHAT_EVENT, ev);
                    }
                    record_update(&notification, &db_notif, &cur_id_notif, &turn_notif).await;
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
                    emit_state(&app_perm, &aid_perm, "permission", Some(title.clone()));
                    let _ = app_perm.emit(PERMISSION_EVENT, json!({
                        "agent_id": aid_perm, "request_id": request_id, "title": title, "options": options_json,
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
                let _init = match conn
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task().await
                {
                    Ok(r) => r,
                    Err(e) => {
                        set_status(&status_main, AcpStatus::Error { message: format!("initialize failed: {e}") });
                        emit_state(&app_main, &aid_main, "error", Some("initialize failed".into()));
                        return Ok(());
                    }
                };

                let mut acp_session: SessionId = match conn
                    .send_request(NewSessionRequest::new(cwd_main.clone()))
                    .block_task().await
                {
                    Ok(r) => r.session_id,
                    Err(e) => {
                        let msg = e.to_string();
                        let (st, state) = if is_auth_required(&msg) {
                            (AcpStatus::AuthRequired { message: format!("{} login required.", aid_main) }, "auth_required")
                        } else {
                            (AcpStatus::Error { message: format!("session/new failed: {msg}") }, "error")
                        };
                        set_status(&status_main, st);
                        emit_state(&app_main, &aid_main, state, None);
                        return Ok(());
                    }
                };

                let session_id_str = acp_session.0.to_string();
                tracing::info!("[{aid_main}] handshake complete (session {session_id_str})");
                set_status(&status_main, AcpStatus::Connected { session_id: session_id_str });
                match db_main.create_session(&aid_main, Some(&workdir), None, None).await {
                    Ok(id) => { if let Ok(mut g) = cur_id_main.lock() { *g = Some(id); } }
                    Err(e) => tracing::error!("[{aid_main}] create_session failed: {e}"),
                }
                emit_state(&app_main, &aid_main, "idle", None);

                let mut rx = cmd_rx;
                while let Some(cmd) = rx.recv().await {
                    match cmd {
                        AcpCommand::Prompt(text) => {
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
                            let req = PromptRequest::new(acp_session.clone(), vec![ContentBlock::Text(TextContent::new(full_text))]);
                            match conn.send_request(req).block_task().await {
                                Ok(_) => {
                                    let agent_text = turn_main.lock().map(|t| t.clone()).unwrap_or_default();
                                    if !agent_text.is_empty() {
                                        if let Some(id) = cur_id(&cur_id_main) {
                                            let _ = db_main.append_event(&id, "agent_message", &json!({"text": agent_text}).to_string()).await;
                                        }
                                    }
                                    emit_state(&app_main, &aid_main, "completed", None);
                                }
                                Err(e) => {
                                    tracing::warn!("[{aid_main}] prompt failed: {e}");
                                    emit_state(&app_main, &aid_main, "error", Some("prompt failed".into()));
                                }
                            }
                        }
                        AcpCommand::NewSession => {
                            if let Some(id) = cur_id(&cur_id_main) {
                                summarize_and_finish(&conn, &acp_session, &db_main, &id, &turn_main, &app_main, &aid_main).await;
                            }
                            match open_session(&conn, &cwd_main, &db_main, &aid_main, &workdir, None).await {
                                Some((new_acp, new_id)) => {
                                    acp_session = new_acp;
                                    if let Ok(mut g) = cur_id_main.lock() { *g = Some(new_id); }
                                    if let Ok(mut g) = ctx_main.lock() { *g = None; }
                                    let _ = app_main.emit(SESSION_RESET_EVENT, json!({"agent_id": aid_main}));
                                    emit_state(&app_main, &aid_main, "idle", None);
                                }
                                None => emit_state(&app_main, &aid_main, "error", Some("new session failed".into())),
                            }
                        }
                        AcpCommand::Resume(prev_id) => {
                            if let Some(id) = cur_id(&cur_id_main) {
                                summarize_and_finish(&conn, &acp_session, &db_main, &id, &turn_main, &app_main, &aid_main).await;
                            }
                            let prev_summary = db_main.get_session(&prev_id).await.ok().flatten().and_then(|r| r.summary);
                            match open_session(&conn, &cwd_main, &db_main, &aid_main, &workdir, Some(&prev_id)).await {
                                Some((new_acp, new_id)) => {
                                    acp_session = new_acp;
                                    if let Ok(mut g) = cur_id_main.lock() { *g = Some(new_id); }
                                    if let Ok(mut g) = ctx_main.lock() {
                                        *g = prev_summary.map(|s| format!("{RESUME_PREFIX}{s}"));
                                    }
                                    let _ = app_main.emit(SESSION_RESET_EVENT, json!({"agent_id": aid_main, "resumed_from": prev_id}));
                                    emit_state(&app_main, &aid_main, "idle", None);
                                }
                                None => emit_state(&app_main, &aid_main, "error", Some("resume failed".into())),
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
                        emit_state(&app, &agent_id, "exited", None);
                    }
                }
            }
            Err(e) => {
                tracing::error!("[{agent_id}] connection ended: {e}");
                if let Ok(mut g) = status.lock() {
                    if !matches!(*g, AcpStatus::AuthRequired { .. } | AcpStatus::Error { .. }) {
                        *g = AcpStatus::Exited { message: e.to_string() };
                        emit_state(&app, &agent_id, "exited", None);
                    }
                }
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
async fn summarize_and_finish(
    conn: &ConnectionTo<Agent>,
    acp_session: &SessionId,
    db: &Db,
    db_id: &str,
    turn_text: &Arc<Mutex<String>>,
    app: &AppHandle,
    agent_id: &str,
) {
    if let Ok(mut t) = turn_text.lock() { t.clear(); }
    emit_state(app, agent_id, "thinking", Some("summarizing".into()));
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
            tracing::warn!("[{agent_id}] summary prompt failed: {e}");
            None
        }
    };
    if let Err(e) = db.finish_session(db_id, summary.as_deref(), "completed").await {
        tracing::warn!("[{agent_id}] finish_session failed: {e}");
    }
}

/// Open a fresh ACP session on the existing connection and create its DB row.
async fn open_session(
    conn: &ConnectionTo<Agent>,
    cwd: &std::path::Path,
    db: &Db,
    agent_id: &str,
    workdir: &str,
    parent: Option<&str>,
) -> Option<(SessionId, String)> {
    let acp = match conn
        .send_request(NewSessionRequest::new(cwd.to_path_buf()))
        .block_task().await
    {
        Ok(r) => r.session_id,
        Err(e) => {
            tracing::warn!("[{agent_id}] session/new failed: {e}");
            return None;
        }
    };
    let db_id = db.create_session(agent_id, Some(workdir), None, parent).await.ok()?;
    Some((acp, db_id))
}

async fn record_update(
    notification: &SessionNotification,
    db: &Db,
    cur_id_state: &Arc<Mutex<Option<String>>>,
    turn_text: &Arc<Mutex<String>>,
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
        })),
        "tool_call_update" => Some(json!({
            "kind": "tool_update",
            "tool_call_id": update.get("toolCallId"),
            "status": update.get("status"),
        })),
        "plan" => Some(json!({ "kind": "plan" })),
        _ => None,
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
