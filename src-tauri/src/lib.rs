//! Kaggle TPU Companion — native Windows shell (Tauri 2) around the existing
//! kaggle-tpu-lab launcher flow.
//!
//! Layout:
//!   state/     application state + pure session state machine + settings
//!   kaggle/    launcher reuse, ntfy events, endpoint probe, orchestration
//!   process/   safe, specific process helpers (no arbitrary shell bridge)
//!   security/  secret hygiene
//!   commands/  the only surface the frontend can reach
//!   tray.rs    system tray (icon, tooltip, menu)

mod commands;
mod kaggle;
mod process;
mod security;
mod state;
mod tray;

use state::AppState;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            crate::tray::show_companion_panel(app, None, true);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_session_state,
            commands::refresh_status,
            commands::start_tpu,
            commands::stop_tpu,
            commands::open_kaggle,
            commands::copy_endpoint,
            commands::copy_api_key,
            commands::copy_model_name,
            commands::copy_connection_setup,
            commands::get_settings,
            commands::save_settings,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // Resolve the launcher (repo + venv python).
            let settings_root = crate::state::settings::Settings::load().project_root.clone();
            let launcher = kaggle::launcher::Launcher::resolve(settings_root.as_deref())
                .expect("could not resolve the kaggle-tpu-lab launcher (launch.py + .venv)");

            let state_file = AppState::default_state_file();
            app.manage(AppState::new(
                launcher,
                state_file,
                Box::new(kaggle::CliKaggleApi(
                    kaggle::launcher::Launcher::resolve(settings_root.as_deref()).expect("launcher"),
                )),
                Box::new(kaggle::NtfySource::default()),
                Box::new(kaggle::HttpProbe::default()),
            ));

            tray::setup_tray(&handle)?;

            // Single polling loop (phase-dependent intervals).
            kaggle::start_poller(handle.clone());

            // Companion behavior: close or focus loss hides the flyout while
            // the tray process keeps running.
            if let Some(window) = app.get_webview_window("main") {
                let w2 = window.clone();
                window.on_window_event(move |event| {
                    match event {
                        tauri::WindowEvent::CloseRequested { api, .. } => {
                            let _ = w2.hide();
                            api.prevent_close();
                        }
                        tauri::WindowEvent::Focused(false) => {
                            let _ = w2.hide();
                        }
                        _ => {}
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the companion");
}
