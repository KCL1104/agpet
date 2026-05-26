//! Dynamic click-through for the transparent overlay.
//!
//! While click-through is on, the webview receives no mouse events, so it can't
//! tell when the cursor enters the pet. We instead poll the global cursor from
//! the backend and toggle `set_ignore_cursor_events`: the window is interactive
//! only while the cursor is over the pet (reported by the frontend each frame)
//! or the chat panel is open; otherwise clicks pass through to the desktop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager, WebviewWindow};

/// Pet bounding box in CSS pixels relative to the window, reported by the frontend.
#[derive(Default, Clone, Copy)]
pub struct PetRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub valid: bool,
}

/// Tauri-managed overlay state shared between commands and the poll loop.
pub struct OverlayState {
    pub pet_rect: Mutex<PetRect>,
    pub panel_open: AtomicBool,
}

impl OverlayState {
    pub fn new() -> Self {
        Self {
            pet_rect: Mutex::new(PetRect::default()),
            panel_open: AtomicBool::new(false),
        }
    }
}

impl Default for OverlayState {
    fn default() -> Self {
        Self::new()
    }
}

/// Poll the cursor ~30x/sec and toggle click-through accordingly.
pub fn spawn_clickthrough_loop(app: AppHandle) {
    std::thread::spawn(move || {
        let Some(window) = app.get_webview_window("main") else {
            tracing::error!("clickthrough loop: main window missing");
            return;
        };
        // Matches the initial `set_ignore_cursor_events(true)` in setup.
        let mut current_ignore = true;

        loop {
            std::thread::sleep(Duration::from_millis(33));

            let state = app.state::<OverlayState>();
            let panel_open = state.panel_open.load(Ordering::Relaxed);
            let over_pet = !panel_open && cursor_over_pet(&window, &state);
            let desired_ignore = !(panel_open || over_pet);

            if desired_ignore != current_ignore {
                match window.set_ignore_cursor_events(desired_ignore) {
                    Ok(()) => current_ignore = desired_ignore,
                    Err(e) => tracing::warn!("set_ignore_cursor_events failed: {e}"),
                }
            }
        }
    });
}

/// True if the global cursor is within the pet's (padded) screen rect.
fn cursor_over_pet(window: &WebviewWindow, state: &OverlayState) -> bool {
    let rect = match state.pet_rect.lock() {
        Ok(g) => *g,
        Err(_) => return false,
    };
    if !rect.valid {
        return false;
    }
    let (Ok(cursor), Ok(origin), Ok(scale)) = (
        window.cursor_position(),
        window.outer_position(),
        window.scale_factor(),
    ) else {
        return false;
    };

    // Pet rect is CSS px relative to the window; convert to global physical px.
    let pad = 10.0 * scale; // easier to hover
    let left = origin.x as f64 + rect.x * scale - pad;
    let top = origin.y as f64 + rect.y * scale - pad;
    let right = left + rect.w * scale + pad * 2.0;
    let bottom = top + rect.h * scale + pad * 2.0;

    cursor.x >= left && cursor.x <= right && cursor.y >= top && cursor.y <= bottom
}
