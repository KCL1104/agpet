mod acp;
mod db;
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

#[tauri::command]
fn send_prompt(instance: String, text: String, state: tauri::State<'_, acp::AcpManager>) -> Result<(), String> {
    state.send_prompt(&instance, text)
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
            send_prompt,
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
                let database = tauri::async_runtime::block_on(db::Db::init(&db_path))?;
                tracing::info!("session DB: {}", db_path.display());
                let config = acp::AgentsConfig::load(app.handle())?;
                tracing::info!("loaded {} agent type(s)", config.agents.len());
                let manager = acp::AcpManager::new(
                    std::sync::Arc::new(database),
                    &config,
                    app.handle().clone(),
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
