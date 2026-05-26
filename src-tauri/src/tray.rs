//! System tray: open the launcher (choose agent type + working dir), close
//! running instances, and quit.
//!
//! Menu layout:
//!   Open Launcher…   (→ emits `open-launcher`; the frontend shows the launcher)
//!   ──────
//!   Close <instance name>  (one per running instance → stop its adapter)
//!   ──────
//!   Quit agpet
//!
//! The menu is rebuilt after each launch/close so the running list stays current.

use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::acp::AcpManager;

const TRAY_ID: &str = "agpet-tray";

pub fn build_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("agpet — desktop agent pets")
        .menu(&menu)
        .on_menu_event(handle_event);
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let manager = app.state::<AcpManager>();

    let launcher = MenuItem::with_id(app, "launcher", "Open Launcher…", true, None::<&str>)?;
    let workflow = MenuItem::with_id(app, "workflow", "Run Workflow…", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let close_items: Vec<MenuItem<R>> = manager
        .list_instances()
        .into_iter()
        .map(|i| {
            MenuItem::with_id(app, format!("close:{}", i.instance_id), format!("Close {}", i.name), true, None::<&str>)
        })
        .collect::<tauri::Result<_>>()?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit agpet", true, None::<&str>)?;

    let mut refs: Vec<&dyn IsMenuItem<R>> = vec![&launcher, &workflow, &sep1];
    for it in &close_items {
        refs.push(it as &dyn IsMenuItem<R>);
    }
    refs.push(&sep2);
    refs.push(&quit);
    Menu::with_items(app, &refs)
}

/// Rebuild the tray menu (call after instances are launched/closed elsewhere).
pub fn refresh<R: Runtime>(app: &AppHandle<R>) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(menu) = build_menu(app) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

fn handle_event<R: Runtime>(app: &AppHandle<R>, event: tauri::menu::MenuEvent) {
    let id = event.id().as_ref().to_string();
    if id == "quit" {
        app.exit(0);
        return;
    }
    if id == "launcher" {
        let _ = app.emit("open-launcher", ());
        return;
    }
    if id == "workflow" {
        let _ = app.emit("open-workflows", ());
        return;
    }
    if let Some(instance_id) = id.strip_prefix("close:") {
        let manager = app.state::<AcpManager>();
        if let Err(e) = manager.close(instance_id) {
            tracing::warn!("tray close failed: {e}");
        }
        refresh(app);
    }
}
