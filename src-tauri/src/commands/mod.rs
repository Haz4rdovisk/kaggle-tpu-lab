//! Tauri commands exposed to the frontend.
//!
//! Security model: the frontend can only call these named commands. There is
//! no `run_command`-style bridge; every argument vector is assembled inside
//! Rust from validated settings.

use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;

use crate::kaggle;
use crate::pi;
use crate::state::{AppState, SessionSnapshot, now_secs};

/// Current sanitized session state (never contains the api key).
#[tauri::command]
pub fn get_session_state(app: AppHandle) -> Result<SessionSnapshot, String> {
    Ok(app.state::<AppState>().snapshot(now_secs()))
}

/// Force an immediate poll cycle and return the fresh snapshot.
#[tauri::command]
pub async fn refresh_status(app: AppHandle) -> Result<SessionSnapshot, String> {
    kaggle::request_refresh(&app);
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || kaggle::tick(&app2))
        .await
        .map_err(|e| e.to_string())
}

/// Start a new session — or reconnect to an existing QUEUED/RUNNING one.
/// Double-start is impossible by construction (Kaggle status is consulted
/// before any push, and a start lock serializes concurrent clicks).
#[tauri::command]
pub async fn start_tpu(app: AppHandle) -> Result<SessionSnapshot, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || kaggle::start_session(&app2))
        .await
        .map_err(|e| e.to_string())?
}

/// Stop the session via the official `launch.py stop` flow. The UI must
/// confirm before calling this; the command itself refuses when there is no
/// active session.
#[tauri::command]
pub async fn stop_tpu(app: AppHandle) -> Result<SessionSnapshot, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || kaggle::stop_session(&app2))
        .await
        .map_err(|e| e.to_string())?
}

/// Sync the live endpoint into the Pi `kaggle-tpu` provider (with rollback).
#[tauri::command]
pub async fn sync_pi_cmd(app: AppHandle) -> Result<String, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || kaggle::sync_pi(&app2))
        .await
        .map_err(|e| e.to_string())?
}

/// Pi integration status (separate from the TPU state).
#[tauri::command]
pub fn get_pi_status(app: AppHandle) -> Result<pi::PiState, String> {
    let st = app.state::<AppState>();
    let endpoint = st.machine.lock().unwrap().endpoint.clone();
    Ok(pi::assess_current(endpoint.as_deref()))
}

/// Open the active kernel's page in the default browser.
#[tauri::command]
pub fn open_kaggle(app: AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    let kernel = st
        .machine
        .lock()
        .unwrap()
        .kernel
        .clone()
        .ok_or_else(|| "No kernel known yet".to_string())?;
    let url = format!("https://www.kaggle.com/code/{kernel}");
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

/// Copy the public endpoint (never the key).
#[tauri::command]
pub fn copy_endpoint(app: AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    let endpoint = st
        .machine
        .lock()
        .unwrap()
        .endpoint
        .clone()
        .ok_or_else(|| "No endpoint yet".to_string())?;
    app.clipboard()
        .write_text(&endpoint)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<crate::state::settings::Settings, String> {
    Ok(app.state::<AppState>().settings.lock().unwrap().clone())
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    settings: crate::state::settings::Settings,
) -> Result<crate::state::settings::Settings, String> {
    let st = app.state::<AppState>();
    settings.validate()?;
    if let Some(root) = &settings.project_root {
        if !std::path::Path::new(root).is_dir() {
            return Err(format!("project_root does not exist: {root}"));
        }
    }
    let mut guard = st.settings.lock().unwrap();
    *guard = settings.clone();
    drop(guard);
    settings.save()?;
    Ok(settings)
}
