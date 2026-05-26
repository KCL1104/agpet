mod acp;
mod db;
mod overlay;

use std::sync::atomic::Ordering;

use tauri::{Manager, PhysicalPosition, PhysicalSize};

use overlay::{OverlayState, PetRect};

/// Current ACP connection status, for the frontend / debugging.
#[tauri::command]
fn acp_status(state: tauri::State<'_, acp::AcpManager>) -> acp::AcpStatus {
    state.status()
}

/// Send a user prompt to the live ACP session.
#[tauri::command]
fn send_prompt(text: String, state: tauri::State<'_, acp::AcpManager>) -> Result<(), String> {
    state.send_prompt(text)
}

/// Resolve a pending permission request with the user's chosen option id.
#[tauri::command]
fn respond_permission(id: String, choice: String, state: tauri::State<'_, acp::AcpManager>) {
    state.respond_permission(id, choice);
}

/// Frontend reports the pet's bounding box (CSS px) so the backend can hit-test
/// the cursor against it for click-through toggling.
#[tauri::command]
fn update_pet_rect(x: f64, y: f64, w: f64, h: f64, state: tauri::State<'_, OverlayState>) {
    if let Ok(mut r) = state.pet_rect.lock() {
        *r = PetRect {
            x,
            y,
            w,
            h,
            valid: true,
        };
    }
}

/// Frontend tells the backend whether the chat panel is open (forces the window
/// interactive while open).
#[tauri::command]
fn set_panel_open(open: bool, state: tauri::State<'_, OverlayState>) {
    state.panel_open.store(open, Ordering::Relaxed);
}

/// End + summarize the current session and open a fresh one.
#[tauri::command]
fn new_session(state: tauri::State<'_, acp::AcpManager>) -> Result<(), String> {
    state.new_session()
}

/// Open a fresh session seeded with a past session's summary (by DB id).
#[tauri::command]
fn resume_session(id: String, state: tauri::State<'_, acp::AcpManager>) -> Result<(), String> {
    state.resume_session(id)
}

/// List recent persisted sessions for the history panel.
#[tauri::command]
async fn list_sessions(
    state: tauri::State<'_, acp::AcpManager>,
) -> Result<Vec<db::SessionRow>, String> {
    let db = state.db_handle();
    db.list_sessions(50).await.map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            acp_status,
            send_prompt,
            respond_permission,
            update_pet_rect,
            set_panel_open,
            new_session,
            resume_session,
            list_sessions
        ])
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .expect("main window should exist");

            // Stretch the transparent window to cover the primary monitor so the
            // pet can roam the whole desktop as an overlay.
            if let Ok(Some(monitor)) = window.primary_monitor() {
                let pos = monitor.position();
                let size = monitor.size();
                let _ = window.set_position(PhysicalPosition::new(pos.x, pos.y));
                let _ = window.set_size(PhysicalSize::new(size.width, size.height));
            }

            // Start fully click-through; the overlay loop toggles this on/off as
            // the cursor enters/leaves the pet (M1 step 4).
            let _ = window.set_ignore_cursor_events(true);

            // Overlay click-through: manage shared state, then poll the cursor.
            app.manage(OverlayState::new());
            overlay::spawn_clickthrough_loop(app.handle().clone());

            // ACP client + persistence: open the SQLite DB, then spawn
            // claude-code-acp, run the handshake, stream session/update -> pet
            // states + chat events, accept prompts, and persist sessions.
            // Failures are logged and degrade gracefully (no crash).
            acp::logging::init();
            match app.path().app_data_dir() {
                Ok(dir) => {
                    let db_path = dir.join("agpet.db");
                    match tauri::async_runtime::block_on(db::Db::init(&db_path)) {
                        Ok(database) => {
                            let manager = acp::AcpManager::new(std::sync::Arc::new(database));
                            match acp::AcpConfig::default_for(app.handle()) {
                                Ok(config) => manager.start(app.handle().clone(), config),
                                Err(e) => tracing::error!("ACP config error, adapter not started: {e:#}"),
                            }
                            app.manage(manager);
                            tracing::info!("session DB: {}", db_path.display());
                        }
                        Err(e) => tracing::error!("DB init failed, ACP disabled: {e:#}"),
                    }
                }
                Err(e) => tracing::error!("app_data_dir failed, ACP disabled: {e}"),
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
