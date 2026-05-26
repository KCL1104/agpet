//! Dynamic click-through for the transparent overlay (multi-pet).
//!
//! While click-through is on, the webview gets no mouse events, so the backend
//! polls the global cursor and toggles `set_ignore_cursor_events`: the window is
//! interactive only while the cursor is over *some* reported rect — a pet or an
//! open panel (both reported by the frontend) — or while a drag is in progress;
//! otherwise clicks pass through to the desktop.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;
use tauri::{AppHandle, Manager, WebviewWindow};

/// An interactive bounding box (pet or open panel) in CSS pixels relative to
/// the window.
#[derive(Clone, Copy)]
pub struct PetRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Frontend payload for reporting an interactive rect (pet instance id or a
/// synthetic panel id like `__panel:chat`).
#[derive(Deserialize)]
pub struct PetRectInput {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub struct OverlayState {
    pub pet_rects: Mutex<HashMap<String, PetRect>>,
    /// True while the user is dragging a pet or the chat panel. Forces the window
    /// interactive so a fast drag can't outrun the (≤40ms-stale) rect and flip
    /// click-through on mid-gesture.
    pub dragging: AtomicBool,
}

impl OverlayState {
    pub fn new() -> Self {
        Self {
            pet_rects: Mutex::new(HashMap::new()),
            dragging: AtomicBool::new(false),
        }
    }

    pub fn set_rects(&self, rects: Vec<PetRectInput>) {
        if let Ok(mut map) = self.pet_rects.lock() {
            map.clear();
            for r in rects {
                map.insert(r.id, PetRect { x: r.x, y: r.y, w: r.w, h: r.h });
            }
        }
    }
}

impl Default for OverlayState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn spawn_clickthrough_loop(app: AppHandle) {
    std::thread::spawn(move || {
        let Some(window) = app.get_webview_window("main") else {
            tracing::error!("clickthrough loop: main window missing");
            return;
        };
        let mut current_ignore = true;
        loop {
            std::thread::sleep(Duration::from_millis(33));
            let state = app.state::<OverlayState>();
            let dragging = state.dragging.load(Ordering::Relaxed);
            let over = !dragging && cursor_over_any_rect(&window, &state);
            let desired_ignore = !(dragging || over);
            if desired_ignore != current_ignore {
                match window.set_ignore_cursor_events(desired_ignore) {
                    Ok(()) => current_ignore = desired_ignore,
                    Err(e) => tracing::warn!("set_ignore_cursor_events failed: {e}"),
                }
            }
        }
    });
}

fn cursor_over_any_rect(window: &WebviewWindow, state: &OverlayState) -> bool {
    let (Ok(cursor), Ok(origin), Ok(scale)) = (
        window.cursor_position(),
        window.outer_position(),
        window.scale_factor(),
    ) else {
        return false;
    };
    let Ok(map) = state.pet_rects.lock() else { return false };
    let pad = 10.0 * scale;
    for r in map.values() {
        let left = origin.x as f64 + r.x * scale - pad;
        let top = origin.y as f64 + r.y * scale - pad;
        let right = left + r.w * scale + pad * 2.0;
        let bottom = top + r.h * scale + pad * 2.0;
        if cursor.x >= left && cursor.x <= right && cursor.y >= top && cursor.y <= bottom {
            return true;
        }
    }
    false
}
