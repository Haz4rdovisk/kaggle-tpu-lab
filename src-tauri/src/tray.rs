//! System tray: icon, tooltip and menu, all driven by the session phase.
//!
//! Visual language: one glyph (TPU chip) with a semantic status dot.
//! Discreet and consistent: idle/stopped share a neutral dot, queued and
//! starting share amber, ready is green, error is red.

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::machine::TpuPhase;
use crate::state::{format_compact, SessionSnapshot};

pub const TRAY_ID: &str = "main-tray";

const TRAY_IDLE: &[u8] = include_bytes!("../icons/tray_idle.png");
const TRAY_QUEUED: &[u8] = include_bytes!("../icons/tray_queued.png");
const TRAY_STARTING: &[u8] = include_bytes!("../icons/tray_starting.png");
const TRAY_READY: &[u8] = include_bytes!("../icons/tray_ready.png");
const TRAY_ERROR: &[u8] = include_bytes!("../icons/tray_error.png");
const TRAY_STOPPED: &[u8] = include_bytes!("../icons/tray_stopped.png");

fn icon_for(phase: TpuPhase) -> Result<Image<'static>, String> {
    let bytes = match phase {
        TpuPhase::Verifying | TpuPhase::Queued => TRAY_QUEUED,
        TpuPhase::Provisioning
        | TpuPhase::Starting
        | TpuPhase::LoadingWeights
        | TpuPhase::Compiling
        | TpuPhase::Healthy
        | TpuPhase::Stopping => TRAY_STARTING,
        TpuPhase::Ready => TRAY_READY,
        TpuPhase::Failed => TRAY_ERROR,
        TpuPhase::Idle => TRAY_IDLE,
        TpuPhase::Stopped => TRAY_STOPPED,
    };
    Image::from_bytes(bytes).map_err(|e| e.to_string())
}

fn tooltip(snap: &SessionSnapshot) -> String {
    let mut line1 = snap
        .model
        .map(|m| m.tray_name())
        .unwrap_or("Kaggle TPU")
        .to_string();
    let line2 = match snap.phase {
        TpuPhase::Ready => match snap.remaining_secs {
            Some(secs) => format!("READY \u{b7} {} remaining", format_compact(secs)),
            None => "READY".to_string(),
        },
        TpuPhase::Verifying => "VERIFYING KAGGLE".to_string(),
        TpuPhase::Queued => "WAITING FOR TPU".to_string(),
        TpuPhase::Provisioning
        | TpuPhase::Starting
        | TpuPhase::LoadingWeights
        | TpuPhase::Compiling => "STARTING".to_string(),
        TpuPhase::Healthy => "ALMOST READY".to_string(),
        TpuPhase::Stopping => "STOPPING".to_string(),
        TpuPhase::Failed => "ERROR".to_string(),
        TpuPhase::Idle => "IDLE".to_string(),
        TpuPhase::Stopped => "STOPPED".to_string(),
    };
    line1.push_str("\n");
    line1.push_str(&line2);
    line1
}

fn status_line(snap: &SessionSnapshot) -> String {
    match snap.phase {
        TpuPhase::Ready => {
            if let Some(secs) = snap.remaining_secs {
                format!("\u{25cf} READY \u{b7} {} left", format_compact(secs))
            } else {
                "\u{25cf} READY".to_string()
            }
        }
        TpuPhase::Queued => "\u{25cf} WAITING FOR TPU".to_string(),
        TpuPhase::Healthy => "\u{25cf} ALMOST READY".to_string(),
        other => format!("\u{25cf} {}", other.label()),
    }
}

fn build_menu(app: &AppHandle, snap: &SessionSnapshot) -> tauri::Result<Menu<tauri::Wry>> {
    let title_text = snap
        .model
        .map(|m| m.tray_name())
        .unwrap_or("Kaggle TPU");
    let title = MenuItem::with_id(app, "title", title_text, false, None::<&str>)?;
    let status = MenuItem::with_id(app, "status", status_line(snap), false, None::<&str>)?;
    let open_panel = MenuItem::with_id(app, "open-panel", "Open panel", true, None::<&str>)?;
    let start = MenuItem::with_id(
        app,
        "start-tpu",
        "Start TPU",
        snap.phase.can_start(),
        None::<&str>,
    )?;
    let stop = MenuItem::with_id(
        app,
        "stop-tpu",
        "Stop TPU",
        snap.phase.can_stop(),
        None::<&str>,
    )?;
    let pi_label = if !snap.pi_sync_supported {
        "Pi: Qwen only"
    } else {
        match snap.pi_status {
            crate::pi::PiState::Synced => "Pi: ready",
            crate::pi::PiState::Stale => "Pi: re-sync needed",
            crate::pi::PiState::SyncFailed => "Pi: sync failed",
            crate::pi::PiState::NotConfigured => "Sync with Pi",
        }
    };
    let pi_item = MenuItem::with_id(app, "sync-pi", pi_label, snap.pi_sync_supported, None::<&str>)?;
    let open_kaggle = MenuItem::with_id(
        app,
        "open-kaggle",
        "Open Kaggle",
        snap.kernel.is_some(),
        None::<&str>,
    )?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &title,
            &PredefinedMenuItem::separator(app)?,
            &status,
            &open_panel,
            &PredefinedMenuItem::separator(app)?,
            &start,
            &stop,
            &pi_item,
            &PredefinedMenuItem::separator(app)?,
            &open_kaggle,
            &settings,
            &quit,
        ],
    )
}

fn open_panel(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.set_always_on_top(false);
        crate::kaggle::request_refresh(app);
    }
}

fn handle_menu(app: &AppHandle, id: &str) {
    match id {
        "open-panel" => open_panel(app),
        "start-tpu" => {
            let app2 = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = tauri::async_runtime::spawn_blocking(move || crate::kaggle::start_session(&app2)).await;
            });
        }
        "stop-tpu" => {
            // Tray stop goes through the same confirmation as the panel.
            open_panel(app);
            if let Err(e) = app.emit("open-stop-confirm", ()) {
                let _ = e;
            }
        }
        "sync-pi" => {
            let app2 = app.clone();
            tauri::async_runtime::spawn(async move {
                let app3 = app2.clone();
                let res = tauri::async_runtime::spawn_blocking(move || crate::kaggle::sync_pi(&app3)).await;
                notify_sync_result(&app2, res);
            });
        }
        "open-kaggle" => {
            if let Err(e) = crate::commands::open_kaggle(app.clone()) {
                let _ = e;
            }
        }
        "settings" => {
            open_panel(app);
            if let Err(e) = app.emit("open-settings", ()) {
                let _ = e;
            }
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

fn notify_sync_result(app: &AppHandle, res: Result<Result<String, String>, tauri::Error>) {
    match res {
        Ok(Ok(msg)) => {
            let snap = app.state::<crate::state::AppState>().snapshot(crate::state::now_secs());
            let _ = crate::tray::update_tray(app, &snap);
            let _ = msg;
        }
        _ => {
            let snap = app.state::<crate::state::AppState>().snapshot(crate::state::now_secs());
            let _ = crate::tray::update_tray(app, &snap);
        }
    }
}

/// Initial tray install (called once from setup).
pub fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let snap = app.state::<crate::state::AppState>().snapshot(crate::state::now_secs());
    let menu = build_menu(app, &snap)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(&tooltip(&snap))
        .icon(icon_for(snap.phase)?)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref().to_string();
            handle_menu(app, &id);
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                open_panel(app);
            }
        })
        .build(app)?;
    Ok(())
}

/// Refresh icon/tooltip/menu to match the snapshot (called on change only).
pub fn update_tray(app: &AppHandle, snap: &SessionSnapshot) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let _ = tray.set_tooltip(Some(&tooltip(snap)));
    if let Ok(icon) = icon_for(snap.phase) {
        let _ = tray.set_icon(Some(icon));
    }
    if let Ok(menu) = build_menu(app, snap) {
        let _ = tray.set_menu(Some(menu));
    }
}
