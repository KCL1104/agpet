mod acp;
mod db;
mod git;
mod mcp;
mod overlay;
mod tray;

use std::sync::atomic::Ordering;

use tauri::{Manager, PhysicalPosition, PhysicalSize};

use overlay::{OverlayState, PetRectInput};

/// Running instances (pets) to render.
#[tauri::command]
fn list_instances(state: tauri::State<'_, acp::AcpManager>) -> Vec<acp::InstanceInfo> {
    state.list_instances()
}

/// Agent types (for a "New instance" menu).
#[tauri::command]
fn list_types(state: tauri::State<'_, acp::AcpManager>) -> Vec<acp::TypeInfo> {
    state.list_types()
}

/// Launch a new instance of an agent type.
#[tauri::command]
fn launch_instance(
    kind: String,
    cwd: Option<String>,
    state: tauri::State<'_, acp::AcpManager>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    let id = state.launch(&kind, cwd)?;
    tray::refresh(&app);
    Ok(id)
}

/// Close (stop) an instance.
#[tauri::command]
fn close_instance(
    instance: String,
    state: tauri::State<'_, acp::AcpManager>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    state.close(&instance)?;
    tray::refresh(&app);
    Ok(())
}

/// Reconnect a stopped/errored instance.
#[tauri::command]
fn retry_agent(instance: String, state: tauri::State<'_, acp::AcpManager>) -> Result<(), String> {
    state.retry(&instance)
}

/// Rename an instance (so delegate/tray see the friendly name); persists the
/// name + mention handle to the pet for companions.
#[tauri::command]
fn rename_instance(
    instance: String,
    name: String,
    handle: String,
    state: tauri::State<'_, acp::AcpManager>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    state.rename(&instance, name, handle)?;
    tray::refresh(&app);
    Ok(())
}

/// Persist a pet's dropped position / roaming height (companions only).
#[tauri::command]
fn set_pet_position(
    instance: String,
    x: Option<f64>,
    custom_y: f64,
    state: tauri::State<'_, acp::AcpManager>,
) -> Result<(), String> {
    state.set_pet_position(&instance, x, custom_y)
}

#[tauri::command]
fn send_prompt(
    instance: String,
    text: String,
    files: Vec<String>,
    images: Vec<acp::PromptImage>,
    state: tauri::State<'_, acp::AcpManager>,
) -> Result<(), String> {
    state.send_prompt(&instance, text, files, images)
}

/// Files under an instance's working dir, for the chat `@`-mention picker.
#[tauri::command]
fn list_dir_files(instance: String, state: tauri::State<'_, acp::AcpManager>) -> Result<Vec<String>, String> {
    state.list_dir_files(&instance)
}

/// Cancel the in-flight prompt turn on an instance (Stop button).
#[tauri::command]
fn cancel_prompt(instance: String, state: tauri::State<'_, acp::AcpManager>) -> Result<(), String> {
    state.cancel(&instance)
}

/// Create a git worktree (new branch off HEAD) in the base instance's repo.
#[tauri::command]
fn worktree_create(
    base_instance: String,
    branch: String,
    state: tauri::State<'_, acp::AcpManager>,
) -> Result<String, String> {
    let repo = state.cwd_of(&base_instance).ok_or_else(|| format!("unknown instance: {base_instance}"))?;
    git::worktree_create(&repo, &branch).map(|p| p.to_string_lossy().into_owned())
}

/// List worktrees agpet created under the base instance's repo.
#[tauri::command]
fn worktree_list(
    base_instance: String,
    state: tauri::State<'_, acp::AcpManager>,
) -> Result<Vec<git::WorktreeInfo>, String> {
    let repo = state.cwd_of(&base_instance).ok_or_else(|| format!("unknown instance: {base_instance}"))?;
    git::worktree_list(&repo)
}

/// Remove a worktree (fails if it has uncommitted changes).
#[tauri::command]
fn worktree_remove(
    base_instance: String,
    path: String,
    state: tauri::State<'_, acp::AcpManager>,
) -> Result<(), String> {
    let repo = state.cwd_of(&base_instance).ok_or_else(|| format!("unknown instance: {base_instance}"))?;
    git::worktree_remove(&repo, &path)
}

/// Merge a worktree's branch into the base instance's current branch.
#[tauri::command]
fn worktree_merge(
    base_instance: String,
    branch: String,
    state: tauri::State<'_, acp::AcpManager>,
) -> Result<String, String> {
    let repo = state.cwd_of(&base_instance).ok_or_else(|| format!("unknown instance: {base_instance}"))?;
    git::worktree_merge(&repo, &branch)
}

/// Stage + commit everything in a worktree (on its own branch).
#[tauri::command]
fn worktree_commit(path: String, message: String) -> Result<String, String> {
    git::worktree_commit(std::path::Path::new(&path), &message)
}

/// Is the base instance's working dir a git repo? (Gate the worktree options.)
#[tauri::command]
fn is_git_repo(base_instance: String, state: tauri::State<'_, acp::AcpManager>) -> bool {
    state.cwd_of(&base_instance).map(|p| git::is_repo(&p)).unwrap_or(false)
}

/// Run a vertical task: sequential handoff over the given worker instance ids.
#[tauri::command]
fn run_handoff(workers: Vec<String>, text: String, state: tauri::State<'_, acp::AcpManager>) -> Result<(), String> {
    state.run_handoff(workers, text)
}

#[tauri::command]
fn respond_permission(instance: String, id: String, choice: String, state: tauri::State<'_, acp::AcpManager>) {
    state.respond_permission(&instance, id, choice);
}

#[tauri::command]
fn new_session(instance: String, state: tauri::State<'_, acp::AcpManager>) -> Result<(), String> {
    state.new_session(&instance)
}

#[tauri::command]
fn resume_session(instance: String, id: String, state: tauri::State<'_, acp::AcpManager>) -> Result<(), String> {
    state.resume_session(&instance, id)
}

#[tauri::command]
fn get_agent_config(instance: String, state: tauri::State<'_, acp::AcpManager>) -> serde_json::Value {
    state.agent_config(&instance)
}

#[tauri::command]
fn set_mode(instance: String, mode: String, state: tauri::State<'_, acp::AcpManager>) -> Result<(), String> {
    state.set_mode(&instance, mode)
}

#[tauri::command]
fn set_model(instance: String, model: String, state: tauri::State<'_, acp::AcpManager>) -> Result<(), String> {
    state.set_model(&instance, model)
}

/// Available workflows (id / name / required agent types).
#[tauri::command]
fn list_workflows(state: tauri::State<'_, acp::AcpManager>) -> Vec<acp::Workflow> {
    state.list_workflows()
}

/// Run a workflow with the given user input across running pets.
#[tauri::command]
fn run_workflow(
    workflow_id: String,
    input: String,
    state: tauri::State<'_, acp::AcpManager>,
) -> Result<(), String> {
    state.run_workflow(&workflow_id, input)
}

/// Recent sessions for the instance's agent type.
#[tauri::command]
async fn list_sessions(
    instance: String,
    state: tauri::State<'_, acp::AcpManager>,
) -> Result<Vec<db::SessionRow>, String> {
    let type_id = state.type_of(&instance).ok_or_else(|| "unknown instance".to_string())?;
    let db = state.db_handle();
    db.list_sessions(&type_id, 50).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn update_pet_rects(rects: Vec<PetRectInput>, state: tauri::State<'_, OverlayState>) {
    state.set_rects(rects);
}

/// Keep the window interactive while a pet or the chat panel is being dragged.
#[tauri::command]
fn set_dragging(dragging: bool, state: tauri::State<'_, OverlayState>) {
    state.dragging.store(dragging, Ordering::Relaxed);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            list_instances,
            list_types,
            launch_instance,
            close_instance,
            retry_agent,
            rename_instance,
            set_pet_position,
            send_prompt,
            list_dir_files,
            cancel_prompt,
            worktree_create,
            worktree_list,
            worktree_remove,
            worktree_merge,
            worktree_commit,
            is_git_repo,
            run_handoff,
            respond_permission,
            new_session,
            resume_session,
            get_agent_config,
            set_mode,
            set_model,
            list_workflows,
            run_workflow,
            list_sessions,
            update_pet_rects,
            set_dragging
        ])
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .expect("main window should exist");

            if let Ok(Some(monitor)) = window.primary_monitor() {
                let pos = monitor.position();
                let size = monitor.size();
                let _ = window.set_position(PhysicalPosition::new(pos.x, pos.y));
                let _ = window.set_size(PhysicalSize::new(size.width, size.height));
            }

            let _ = window.set_ignore_cursor_events(true);
            app.manage(OverlayState::new());
            overlay::spawn_clickthrough_loop(app.handle().clone());

            acp::logging::init();
            let setup: anyhow::Result<()> = (|| {
                let db_path = app.path().app_data_dir()?.join("agpet.db");
                let database = std::sync::Arc::new(tauri::async_runtime::block_on(db::Db::init(&db_path))?);
                tracing::info!("session DB: {}", db_path.display());
                let config = acp::AgentsConfig::load(app.handle())?;
                tracing::info!("loaded {} agent type(s)", config.agents.len());
                // Prime the durable pet identities so launched companions restore
                // their name/position/stats synchronously.
                let companions = tauri::async_runtime::block_on(database.list_companions()).unwrap_or_default();
                tracing::info!("loaded {} companion pet(s)", companions.len());
                // Start agpet's MCP delegate server first so its (url, token) can
                // be attached to each agent session.
                let mcp = match mcp::start(app.handle().clone()) {
                    Ok((url, token)) => { tracing::info!("MCP delegate server: {url}"); Some((url, token)) }
                    Err(e) => { tracing::error!("MCP server failed to start: {e:#}"); None }
                };
                let manager = acp::AcpManager::new(
                    database,
                    &config,
                    app.handle().clone(),
                    mcp,
                    companions,
                );
                // Start with an empty desktop; the user launches instances (with a
                // working dir) from the launcher panel via the tray.
                app.manage(manager);
                tray::build_tray(app.handle())?;
                Ok(())
            })();
            if let Err(e) = setup {
                tracing::error!("ACP setup failed, agents disabled: {e:#}");
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
