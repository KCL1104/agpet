//! System tray: launch new agent instances, close running ones, and quit.
//!
//! Menu layout:
//!   New ▸  (one item per agent type → launch a new instance)
//!   ──────
//!   Close <instance name>  (one per running instance → stop its adapter)
//!   ──────
//!   Quit agpet
//!
//! The menu is rebuilt after each launch/close so the running list stays current.

use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Runtime};

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

    // "New ▸" submenu: one launch item per agent type.
    let new_items: Vec<MenuItem<R>> = manager
        .list_types()
        .into_iter()
        .map(|t| MenuItem::with_id(app, format!("new:{}", t.type_id), &t.name, true, None::<&str>))
        .collect::<tauri::Result<_>>()?;
    let new_refs: Vec<&dyn IsMenuItem<R>> = new_items.iter().map(|i| i as &dyn IsMenuItem<R>).collect();
    let new_sub = Submenu::with_items(app, "New ▸", true, &new_refs)?;

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

    let mut refs: Vec<&dyn IsMenuItem<R>> = vec![&new_sub, &sep1];
    for it in &close_items {
        refs.push(it as &dyn IsMenuItem<R>);
    }
    refs.push(&sep2);
    refs.push(&quit);
    Menu::with_items(app, &refs)
}

fn rebuild<R: Runtime>(app: &AppHandle<R>) {
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
    let manager = app.state::<AcpManager>();
    if let Some(type_id) = id.strip_prefix("new:") {
        if let Err(e) = manager.launch(type_id) {
            tracing::warn!("tray launch failed: {e}");
        }
        rebuild(app);
    } else if let Some(instance_id) = id.strip_prefix("close:") {
        if let Err(e) = manager.close(instance_id) {
            tracing::warn!("tray close failed: {e}");
        }
        rebuild(app);
    }
}
