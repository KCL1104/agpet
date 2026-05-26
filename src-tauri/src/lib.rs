use tauri::{Manager, PhysicalPosition, PhysicalSize};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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

            // Milestone 1, step 1: the pet is non-interactive, so let every click
            // fall through to whatever is behind it. Dynamic toggling (so clicking
            // the pet opens a chat panel) arrives in M1 step 4.
            let _ = window.set_ignore_cursor_events(true);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
