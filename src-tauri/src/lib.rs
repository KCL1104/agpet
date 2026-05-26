mod acp;
mod db;
mod overlay;

use std::sync::atomic::Ordering;

use tauri::{Manager, PhysicalPosition, PhysicalSize};

use overlay::{OverlayState, PetRectInput};

/// Agents to render as pets (id / name / colour).
#[tauri::command]
fn list_agents(state: tauri::State<'_, acp::AcpManager>) -> Vec<acp::AgentInfo> {
    state.list_agents()
}

/// Send a user prompt to a specific agent's live session.
#[tauri::command]
fn send_prompt(agent: String, text: String, state: tauri::State<'_, acp::AcpManager>) -> Result<(), String> {
    state.send_prompt(&agent, text)
}

/// Resolve a pending permission request for an agent.
#[tauri::command]
fn respond_permission(agent: String, id: String, choice: String, state: tauri::State<'_, acp::AcpManager>) {
    state.respond_permission(&agent, id, choice);
}

/// End + summarize an agent's current session and open a fresh one.
#[tauri::command]
fn new_session(agent: String, state: tauri::State<'_, acp::AcpManager>) -> Result<(), String> {
    state.new_session(&agent)
}

/// Open a fresh session for an agent, seeded with a past session's summary.
#[tauri::command]
fn resume_session(agent: String, id: String, state: tauri::State<'_, acp::AcpManager>) -> Result<(), String> {
    state.resume_session(&agent, id)
}

/// List recent persisted sessions for one agent.
#[tauri::command]
async fn list_sessions(
    agent: String,
    state: tauri::State<'_, acp::AcpManager>,
) -> Result<Vec<db::SessionRow>, String> {
    let db = state.db_handle();
    db.list_sessions(&agent, 50).await.map_err(|e| e.to_string())
}

/// Frontend reports every pet's bounding box (CSS px) for click-through hit-testing.
#[tauri::command]
fn update_pet_rects(rects: Vec<PetRectInput>, state: tauri::State<'_, OverlayState>) {
    state.set_rects(rects);
}

/// Frontend tells the backend whether the chat panel is open.
#[tauri::command]
fn set_panel_open(open: bool, state: tauri::State<'_, OverlayState>) {
    state.panel_open.store(open, Ordering::Relaxed);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_agents,
            send_prompt,
            respond_permission,
            new_session,
            resume_session,
            list_sessions,
            update_pet_rects,
            set_panel_open
        ])
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .expect("main window should exist");

            // Stretch the transparent window to cover the primary monitor.
            if let Ok(Some(monitor)) = window.primary_monitor() {
                let pos = monitor.position();
                let size = monitor.size();
                let _ = window.set_position(PhysicalPosition::new(pos.x, pos.y));
                let _ = window.set_size(PhysicalSize::new(size.width, size.height));
            }

            // Start fully click-through; the overlay loop toggles per pet.
            let _ = window.set_ignore_cursor_events(true);
            app.manage(OverlayState::new());
            overlay::spawn_clickthrough_loop(app.handle().clone());

            // Open the session DB, load agents, and spawn one ACP client per agent.
            acp::logging::init();
            let setup: anyhow::Result<()> = (|| {
                let db_path = app.path().app_data_dir()?.join("agpet.db");
                let database = tauri::async_runtime::block_on(db::Db::init(&db_path))?;
                tracing::info!("session DB: {}", db_path.display());
                let config = acp::AgentsConfig::load(app.handle())?;
                tracing::info!("loaded {} agent(s)", config.agents.len());
                let manager = acp::AcpManager::new(std::sync::Arc::new(database), &config);
                manager.start_all(app.handle().clone());
                app.manage(manager);
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
