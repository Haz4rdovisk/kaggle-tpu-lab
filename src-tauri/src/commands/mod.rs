//! Tauri commands exposed to the frontend.
//!
//! Security model: the frontend can only call these named commands. There is
//! no `run_command`-style bridge; every argument vector is assembled inside
//! Rust from validated settings.

use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;

use crate::kaggle;
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
    let endpoint = openai_base_url(&endpoint);
    app.clipboard()
        .write_text(&endpoint)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn copy_api_key(app: AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    let key = st
        .api_key
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "No API key available yet".to_string())?;
    app.clipboard().write_text(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn copy_model_name(app: AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    let model = {
        let m = st.machine.lock().unwrap();
        let settings = st.settings.lock().unwrap();
        m.model.unwrap_or(settings.model).served_name().to_string()
    };
    app.clipboard().write_text(&model).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn copy_connection_setup(app: AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    let (endpoint, model) = {
        let m = st.machine.lock().unwrap();
        let settings = st.settings.lock().unwrap();
        if !m.endpoint_live {
            return Err("Endpoint is not live yet".to_string());
        }
        let endpoint = m.endpoint.clone().ok_or_else(|| "No endpoint yet".to_string())?;
        let model = m.model.unwrap_or(settings.model);
        (openai_base_url(&endpoint), model.served_name())
    };
    let key = st
        .api_key
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "No API key available yet".to_string())?;
    let setup = format!(
        "OPENAI_BASE_URL={endpoint}\nOPENAI_API_KEY={key}\nOPENAI_MODEL={model}"
    );
    app.clipboard().write_text(&setup).map_err(|e| e.to_string())
}

fn openai_base_url(endpoint: &str) -> String {
    let root = endpoint.trim_end_matches('/');
    if root.ends_with("/v1") {
        root.to_string()
    } else {
        format!("{root}/v1")
    }
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
