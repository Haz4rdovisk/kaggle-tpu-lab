//! System tray: icon, tooltip and menu, all driven by the session phase.
//!
//! Visual language: one glyph (TPU chip) with a semantic status dot.
//! Discreet and consistent: idle/stopped share a neutral dot, queued and
//! starting share amber, ready is green, error is red.

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewWindow};

use crate::state::machine::TpuPhase;
use crate::state::{format_compact, SessionSnapshot};

pub const TRAY_ID: &str = "main-tray";
const PANEL_WIDTH: f64 = 520.0;
const PANEL_HEIGHT: f64 = 900.0;
const PANEL_MARGIN: f64 = 8.0;

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
    let copy_setup = MenuItem::with_id(
        app,
        "copy-setup",
        "Copy connection setup",
        snap.endpoint_live && snap.has_api_key,
        None::<&str>,
    )?;
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
            &copy_setup,
            &PredefinedMenuItem::separator(app)?,
            &open_kaggle,
            &settings,
            &quit,
        ],
    )
}

#[derive(Clone, Copy)]
enum TaskbarEdge {
    Top,
    Right,
    Bottom,
    Left,
}

fn monitor_for_anchor(
    window: &WebviewWindow,
    anchor: Option<PhysicalPosition<f64>>,
) -> Option<Monitor> {
    if let Some(point) = anchor {
        if let Ok(monitors) = window.available_monitors() {
            if let Some(monitor) = monitors.into_iter().find(|m| {
                let pos = m.position();
                let size = m.size();
                point.x >= pos.x as f64
                    && point.x < (pos.x + size.width as i32) as f64
                    && point.y >= pos.y as f64
                    && point.y < (pos.y + size.height as i32) as f64
            }) {
                return Some(monitor);
            }
        }
    }

    window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
}

fn taskbar_edge(monitor: &Monitor) -> TaskbarEdge {
    let full_pos = monitor.position();
    let full_size = monitor.size();
    let work = monitor.work_area();

    let full_left = full_pos.x;
    let full_top = full_pos.y;
    let full_right = full_left + full_size.width as i32;
    let full_bottom = full_top + full_size.height as i32;
    let work_left = work.position.x;
    let work_top = work.position.y;
    let work_right = work_left + work.size.width as i32;
    let work_bottom = work_top + work.size.height as i32;

    let gaps = [
        (TaskbarEdge::Top, (work_top - full_top).max(0)),
        (TaskbarEdge::Right, (full_right - work_right).max(0)),
        (TaskbarEdge::Bottom, (full_bottom - work_bottom).max(0)),
        (TaskbarEdge::Left, (work_left - full_left).max(0)),
    ];

    gaps.into_iter()
        .max_by_key(|(_, gap)| *gap)
        .filter(|(_, gap)| *gap > 0)
        .map(|(edge, _)| edge)
        .unwrap_or(TaskbarEdge::Bottom)
}

fn clamp(value: i32, min: i32, max: i32) -> i32 {
    if max < min {
        min
    } else {
        value.max(min).min(max)
    }
}

fn place_companion_panel(
    window: &WebviewWindow,
    anchor: Option<PhysicalPosition<f64>>,
) {
    let Some(monitor) = monitor_for_anchor(window, anchor) else {
        return;
    };

    let scale = monitor.scale_factor();
    let margin = (PANEL_MARGIN * scale).round() as i32;
    let work = monitor.work_area();
    let work_left = work.position.x;
    let work_top = work.position.y;
    let work_right = work_left + work.size.width as i32;
    let work_bottom = work_top + work.size.height as i32;

    let max_width = work.size.width.saturating_sub((margin * 2).max(0) as u32);
    let max_height = work.size.height.saturating_sub((margin * 2).max(0) as u32);
    let width = ((PANEL_WIDTH * scale).round() as u32).min(max_width);
    let height = ((PANEL_HEIGHT * scale).round() as u32).min(max_height);
    let _ = window.set_size(PhysicalSize::new(width, height));

    let anchor_y = anchor
        .map(|p| p.y.round() as i32)
        .unwrap_or(work_bottom - margin);

    let (x, y) = match taskbar_edge(&monitor) {
        TaskbarEdge::Bottom => (
            work_right - width as i32 - margin,
            work_bottom - height as i32 - margin,
        ),
        TaskbarEdge::Top => (
            work_right - width as i32 - margin,
            work_top + margin,
        ),
        TaskbarEdge::Right => (
            work_right - width as i32 - margin,
            clamp(
                anchor_y - height as i32 + margin,
                work_top + margin,
                work_bottom - height as i32 - margin,
            ),
        ),
        TaskbarEdge::Left => (
            work_left + margin,
            clamp(
                anchor_y - height as i32 + margin,
                work_top + margin,
                work_bottom - height as i32 - margin,
            ),
        ),
    };

    let _ = window.set_position(PhysicalPosition::new(x, y));
}

pub fn show_companion_panel(
    app: &AppHandle,
    anchor: Option<PhysicalPosition<f64>>,
    refresh: bool,
) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        place_companion_panel(&w, anchor);
        let _ = w.set_skip_taskbar(true);
        let _ = w.set_always_on_top(true);
        let _ = w.show();
        let _ = w.set_focus();
        if refresh {
            crate::kaggle::request_refresh(app);
        }
    }
}

fn open_panel(app: &AppHandle) {
    show_companion_panel(app, None, true);
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
        "copy-setup" => {
            let _ = crate::commands::copy_connection_setup(app.clone());
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
                position,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                show_companion_panel(app, Some(position), true);
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
