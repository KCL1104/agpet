//! The ACP client core: spawn the adapter, run the initialize + session/new
//! handshake, map streaming `session/update` events to pet states + chat events,
//! accept user prompts via a command loop, and route permission requests to the
//! user. Runs as a single future on Tauri's async runtime.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, TextContent,
};
use agent_client_protocol::{AcpAgent, Agent, ConnectionTo};
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};

use super::config::AcpConfig;
use super::logging::MessageLog;
use super::{AcpCommand, AcpStatus, PendingPermissions};

/// Tauri event carrying the pet's current visual state.
const PET_STATE_EVENT: &str = "pet-state";
/// Tauri event carrying chat transcript content (agent text, thinking, tools).
const CHAT_EVENT: &str = "chat-event";
/// Tauri event asking the user to allow/deny a tool.
const PERMISSION_EVENT: &str = "permission-request";

/// Fallback counter for permission request ids when the tool call has none.
static PERM_COUNTER: AtomicU64 = AtomicU64::new(0);

fn emit_state(app: &AppHandle, state: &str, detail: Option<String>) {
    tracing::debug!(target: "acp.state", "pet -> {state}{}",
        detail.as_deref().map(|d| format!(" ({d})")).unwrap_or_default());
    let _ = app.emit(PET_STATE_EVENT, json!({ "state": state, "detail": detail }));
}

/// Spawn the ACP adapter and drive the handshake + command loop. Returns
/// immediately. Any failure updates `status` rather than panicking.
pub fn start(
    app: AppHandle,
    config: AcpConfig,
    status: Arc<Mutex<AcpStatus>>,
    cmd_rx: mpsc::UnboundedReceiver<AcpCommand>,
    pending: PendingPermissions,
) {
    // The adapter wraps Claude Code, which refuses to open a session if it sees
    // the `CLAUDECODE` env var (its "don't nest Claude Code" guard). Clearing it
    // for our process lets the spawned adapter start its own session. A pet
    // launched from the desktop never has this set; one launched from inside a
    // Claude Code terminal would.
    if std::env::var_os("CLAUDECODE").is_some() {
        std::env::remove_var("CLAUDECODE");
        tracing::info!("cleared inherited CLAUDECODE env var so the adapter can start a session");
    }

    let message_log = match MessageLog::open(&config.message_log_path) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(
                "cannot open ACP message log {}: {e}",
                config.message_log_path.display()
            );
            set_status(&status, AcpStatus::Error {
                message: format!("open message log: {e}"),
            });
            return;
        }
    };
    tracing::info!("ACP message log: {}", config.message_log_path.display());

    let argv = config.argv();
    let cwd = config.cwd.clone();
    tracing::info!("starting ACP adapter: {argv:?} (session cwd {})", cwd.display());

    tauri::async_runtime::spawn(async move {
        let log_for_debug = message_log.clone();
        let agent = match AcpAgent::from_args(argv) {
            Ok(a) => a.with_debug(move |line, dir| log_for_debug.record(dir, line)),
            Err(e) => {
                set_status(&status, AcpStatus::Error {
                    message: format!("build agent command: {e}"),
                });
                emit_state(&app, "error", Some(format!("{e}")));
                return;
            }
        };

        set_status(&status, AcpStatus::Connecting);
        emit_state(&app, "connecting", None);

        let status_main = status.clone();
        let cwd_main = cwd.clone();
        let app_notif = app.clone();
        let app_perm = app.clone();
        let app_main = app.clone();
        let pending_perm = pending.clone();

        let result = agent_client_protocol::Client
            .builder()
            .name("agpet")
            .on_receive_notification(
                async move |notification: SessionNotification, _cx| {
                    if let Some((state, detail)) = map_update(&notification) {
                        emit_state(&app_notif, state, detail);
                    }
                    if let Some(ev) = chat_event(&notification) {
                        let _ = app_notif.emit(CHAT_EVENT, ev);
                    }
                    Ok(())
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                async move |request: RequestPermissionRequest, responder, _connection| {
                    // Ask the user (real Allow/Deny). The handler waits on a
                    // oneshot that the `respond_permission` command fulfils.
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
                    emit_state(&app_perm, "permission", Some(title.clone()));
                    let _ = app_perm.emit(PERMISSION_EVENT, json!({
                        "request_id": request_id,
                        "title": title,
                        "options": options_json,
                    }));

                    // Wait for the user's choice (option id), then respond.
                    let chosen = rx.await.ok();
                    if let Ok(mut m) = pending_perm.lock() {
                        m.remove(&request_id);
                    }
                    let typed = chosen.as_ref().and_then(|opt_id| {
                        request
                            .options
                            .iter()
                            .find(|o| {
                                serde_json::to_value(o)
                                    .ok()
                                    .and_then(|v| {
                                        v.get("optionId")
                                            .and_then(|x| x.as_str())
                                            .map(|s| s == opt_id)
                                    })
                                    .unwrap_or(false)
                            })
                            .map(|o| o.option_id.clone())
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
                // 1) initialize
                let init = match conn
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        set_status(&status_main, AcpStatus::Error {
                            message: format!("initialize failed: {e}"),
                        });
                        emit_state(&app_main, "error", Some("initialize failed".into()));
                        return Ok(());
                    }
                };

                // 2) session/new
                let sess = match conn
                    .send_request(NewSessionRequest::new(cwd_main.clone()))
                    .block_task()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        let msg = e.to_string();
                        let (st, state) = if is_auth_required(&msg) {
                            (
                                AcpStatus::AuthRequired {
                                    message: "Claude login required — run `claude /login`, then restart the pet."
                                        .into(),
                                },
                                "auth_required",
                            )
                        } else {
                            (
                                AcpStatus::Error {
                                    message: format!("session/new failed: {msg}"),
                                },
                                "error",
                            )
                        };
                        set_status(&status_main, st);
                        emit_state(&app_main, state, None);
                        return Ok(());
                    }
                };

                let agent_json = serde_json::to_value(&init).unwrap_or(serde_json::Value::Null);
                let session_id = serde_json::to_value(&sess.session_id)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_else(|| format!("{:?}", sess.session_id));

                tracing::info!("ACP handshake complete (session {session_id})");
                set_status(&status_main, AcpStatus::Connected {
                    session_id: session_id.clone(),
                    agent: agent_json,
                });
                emit_state(&app_main, "idle", None);

                // 3) Command loop: send user prompts into the live session. One
                //    prompt at a time is enough for M1. The loop ends when the
                //    command channel closes (app shutdown).
                let mut rx = cmd_rx;
                while let Some(cmd) = rx.recv().await {
                    match cmd {
                        AcpCommand::Prompt(text) => {
                            tracing::info!("sending prompt ({} chars)", text.len());
                            let req = PromptRequest::new(
                                sess.session_id.clone(),
                                vec![ContentBlock::Text(TextContent::new(text))],
                            );
                            match conn.send_request(req).block_task().await {
                                Ok(resp) => {
                                    let reason = serde_json::to_value(&resp.stop_reason)
                                        .ok()
                                        .and_then(|v| v.as_str().map(str::to_string))
                                        .unwrap_or_default();
                                    tracing::info!("prompt finished (stop_reason {reason})");
                                    emit_state(&app_main, "completed", None);
                                }
                                Err(e) => {
                                    tracing::warn!("prompt failed: {e}");
                                    emit_state(&app_main, "error", Some("prompt failed".into()));
                                }
                            }
                        }
                    }
                }
                Ok(())
            })
            .await;

        // The connect future returned: the child exited or the connection errored.
        match result {
            Ok(()) => {
                if let Ok(mut g) = status.lock() {
                    if matches!(*g, AcpStatus::Connected { .. } | AcpStatus::Connecting) {
                        *g = AcpStatus::Exited {
                            message: "adapter process ended".into(),
                        };
                        emit_state(&app, "exited", None);
                    }
                }
            }
            Err(e) => {
                tracing::error!("ACP connection ended: {e}");
                if let Ok(mut g) = status.lock() {
                    if !matches!(*g, AcpStatus::AuthRequired { .. } | AcpStatus::Error { .. }) {
                        *g = AcpStatus::Exited {
                            message: e.to_string(),
                        };
                        emit_state(&app, "exited", None);
                    }
                }
            }
        }
    });
}

/// Map a `session/update` notification to a pet state + optional detail.
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

/// Build a chat-transcript event for the panel from a `session/update`.
fn chat_event(notification: &SessionNotification) -> Option<serde_json::Value> {
    let v = serde_json::to_value(notification).ok()?;
    let update = v.get("update")?;
    let kind = update.get("sessionUpdate")?.as_str()?;
    match kind {
        "agent_message_chunk" => Some(json!({
            "kind": "agent_message",
            "text": update.get("content").and_then(extract_text).unwrap_or_default(),
        })),
        "agent_thought_chunk" => Some(json!({
            "kind": "agent_thought",
            "text": update.get("content").and_then(extract_text).unwrap_or_default(),
        })),
        "tool_call" => Some(json!({
            "kind": "tool_call",
            "tool_call_id": update.get("toolCallId"),
            "title": update.get("title"),
            "status": update.get("status"),
            "tool_kind": update.get("kind"),
        })),
        "tool_call_update" => Some(json!({
            "kind": "tool_update",
            "tool_call_id": update.get("toolCallId"),
            "status": update.get("status"),
            "output": update.get("rawOutput"),
        })),
        "plan" => Some(json!({ "kind": "plan", "entries": update.get("entries") })),
        _ => None,
    }
}

/// Pull text out of an ACP content value (handles `{text}`, `{content:{…}}`, arrays).
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

/// Title of the tool a permission request is about, for the panel.
fn permission_title(request: &RequestPermissionRequest) -> String {
    serde_json::to_value(request)
        .ok()
        .and_then(|v| {
            v.get("toolCall")
                .and_then(|t| t.get("title"))
                .and_then(|t| t.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "Permission requested".into())
}

fn set_status(status: &Arc<Mutex<AcpStatus>>, new: AcpStatus) {
    if let Ok(mut g) = status.lock() {
        *g = new;
    }
}

/// Heuristic: does this `session/new` error mean the adapter needs a login?
fn is_auth_required(message: &str) -> bool {
    let m = message.to_lowercase();
    m.contains("login") || m.contains("auth") || m.contains("api key") || m.contains("unauthorized")
}
