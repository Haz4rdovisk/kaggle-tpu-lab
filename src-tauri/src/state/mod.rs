//! Application state: the live machine, settings, secrets (in-memory only)
//! and the sanitized snapshot the UI consumes.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64};
use std::sync::Mutex;

use crate::kaggle::launcher::Launcher;
use crate::kaggle::probe::EndpointProbe;
use crate::kaggle::{EventSource, KaggleApi};
use crate::state::machine::{ActivityNote, KaggleStatus, MachineState, TpuPhase};
use crate::state::model::ModelId;
use crate::state::settings::Settings;

pub mod machine;
pub mod model;
pub mod settings;

/// Sanitized view of the session. By construction this struct has no field
/// that can carry the api key — the key never crosses to the frontend.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub phase: TpuPhase,
    pub model: Option<ModelId>,
    pub kaggle_status: Option<String>,
    pub kernel: Option<String>,
    pub endpoint: Option<String>,
    pub endpoint_live: bool,
    pub ready_at: Option<i64>,
    pub ready_at_estimated: bool,
    pub queued_at: Option<i64>,
    pub allocated_at: Option<i64>,
    pub serving_from: Option<i64>,
    pub serving_from_estimated: bool,
    pub keepalive_min: Option<i64>,
    pub uptime_secs: Option<i64>,
    pub remaining_secs: Option<i64>,
    pub remaining_estimated: bool,
    pub decode_tok_s: Option<f64>,
    pub max_model_len: Option<i64>,
    pub mtp_tokens: Option<i64>,
    pub text_only: Option<bool>,
    pub has_api_key: bool,
    pub ntfy_reachable: bool,
    pub activity: Vec<ActivityNote>,
    pub error: Option<String>,
    pub last_update: i64,
}

pub struct AppState {
    pub machine: Mutex<MachineState>,
    pub settings: Mutex<Settings>,
    pub launcher: Launcher,
    pub state_file: PathBuf,
    /// Session api key. Rust memory only: never serialized, logged or sent.
    pub api_key: Mutex<Option<String>>,
    pub kaggle_api: Box<dyn KaggleApi>,
    pub event_source: Box<dyn EventSource>,
    pub endpoint_probe: Box<dyn EndpointProbe>,
    pub starting: AtomicBool,
    pub stopping: AtomicBool,
    /// Re-entrancy guard so a manual refresh never runs a tick in parallel
    /// with the poller's tick (would duplicate ntfy/probe network I/O).
    pub ticking: AtomicBool,
    /// One-shot flag: true once the panel has been revealed after the first
    /// successful tick. The poller shows + focuses the window only on the
    /// first transition (false -> true) and never again, so it cannot steal
    /// focus from other windows on later ticks.
    pub primed: AtomicBool,
    pub last_probe_at: AtomicI64,
    pub last_kaggle_status: Mutex<Option<KaggleStatus>>,
    pub last_snapshot_json: Mutex<String>,
    /// Channel used to request an immediate tick (panel open / tray action).
    pub refresh_tx: Mutex<Option<std::sync::mpsc::Sender<()>>>,
}

impl AppState {
    pub fn new(
        launcher: Launcher,
        state_file: PathBuf,
        kaggle_api: Box<dyn KaggleApi>,
        event_source: Box<dyn EventSource>,
        endpoint_probe: Box<dyn EndpointProbe>,
    ) -> Self {
        let settings = Settings::load();
        Self {
            machine: Mutex::new(MachineState::default()),
            settings: Mutex::new(settings),
            launcher,
            state_file,
            api_key: Mutex::new(None),
            kaggle_api,
            event_source,
            endpoint_probe,
            starting: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            ticking: AtomicBool::new(false),
            primed: AtomicBool::new(false),
            last_probe_at: AtomicI64::new(0),
            last_kaggle_status: Mutex::new(None),
            last_snapshot_json: Mutex::new(String::new()),
            refresh_tx: Mutex::new(None),
        }
    }

    pub fn default_state_file() -> PathBuf {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        PathBuf::from(home).join(".kaggle-tpu-lab.json")
    }

    /// Build the sanitized snapshot from the current machine state.
    pub fn snapshot(&self, now: i64) -> SessionSnapshot {
        let m = self.machine.lock().unwrap();
        let st = self.settings.lock().unwrap();
        let key = self.api_key.lock().unwrap();
        let last_status = *self.last_kaggle_status.lock().unwrap();
        let effective_model = m.model.unwrap_or(st.model);
        let profile_matches = m.model.is_none() || m.model == Some(st.model);
        SessionSnapshot {
            phase: m.phase,
            model: Some(effective_model),
            kaggle_status: last_status.map(|s| format!("{s:?}").to_lowercase()),
            kernel: m.kernel.clone(),
            endpoint: m.endpoint.clone(),
            endpoint_live: m.endpoint_live,
            ready_at: m.ready_at,
            ready_at_estimated: m.ready_at_estimated,
            queued_at: m.queued_at,
            allocated_at: m.allocated_at,
            serving_from: m.serving_from,
            serving_from_estimated: m.serving_from_estimated,
            keepalive_min: m.keepalive_min.or(Some(st.keepalive_min)),
            uptime_secs: m.uptime_secs(now),
            remaining_secs: m.remaining_secs(now),
            remaining_estimated: m.remaining_is_estimated(),
            decode_tok_s: m.decode_tok_s,
            max_model_len: m.max_model_len.or_else(|| profile_matches.then(|| st.selected_context())),
            mtp_tokens: if effective_model == ModelId::Qwen38_27b {
                m.mtp_tokens.or_else(|| profile_matches.then(|| st.qwen.mtp))
            } else {
                None
            },
            text_only: m.text_only.or_else(|| profile_matches.then(|| st.selected_text_only())),
            has_api_key: key.is_some(),
            ntfy_reachable: m.ntfy_reachable,
            activity: m.activity.clone(),
            error: m.error.clone(),
            last_update: now,
        }
    }
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Format seconds as h:mm:ss (for uptime / remaining readouts).
pub fn format_hms(secs: i64) -> String {
    let secs = secs.max(0);
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// Format seconds as a compact human duration ("6h 42m").
pub fn format_compact(secs: i64) -> String {
    let secs = secs.max(0);
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        format!("{}s", secs % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_hms_variants() {
        assert_eq!(format_hms(0), "00:00");
        assert_eq!(format_hms(65), "01:05");
        assert_eq!(format_hms(4722), "01:18:42");
        assert_eq!(format_hms(24078), "06:41:18");
    }

    #[test]
    fn format_compact_variants() {
        assert_eq!(format_compact(24120), "6h 42m");
        assert_eq!(format_compact(900), "15m");
        assert_eq!(format_compact(45), "45s");
    }

    #[test]
    fn snapshot_never_contains_api_key() {
        use crate::kaggle::ntfy::EventSource as _;
        use crate::kaggle::probe::EndpointProbe as _;
        use crate::kaggle::testutil::FakeKaggleApi;
        struct NoEvents;
        impl EventSource for NoEvents {
            fn fetch(&self, _t: &str, _s: i64) -> Result<Vec<crate::state::machine::LauncherEvent>, String> {
                Ok(vec![])
            }
        }
        struct NoProbe;
        impl EndpointProbe for NoProbe {
            fn probe(&self, _e: &str, _k: &str) -> bool {
                true
            }
        }
        let dir = std::env::temp_dir().join(format!("ktl-snap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state_file = dir.join("state.json");
        std::fs::write(
            &state_file,
            r#"{"kernel":"u/k","topic":"ktl-x","api_key":"sk-SUPERSECRET"}"#,
        )
        .unwrap();
        let launcher = Launcher {
            python: dir.join("python.exe"),
            project_root: dir.clone(),
        };
        let st = AppState::new(
            launcher,
            state_file,
            Box::new(FakeKaggleApi::running()),
            Box::new(NoEvents),
            Box::new(NoProbe),
        );
        *st.api_key.lock().unwrap() = Some("sk-SUPERSECRET".into());
        let snap = st.snapshot(1000);
        let json = serde_json::to_string(&snap).unwrap();
        assert!(!json.contains("SUPERSECRET"), "snapshot leaks the key: {json}");
        assert!(!json.contains("sk-"));
        assert!(snap.has_api_key);
    }
}
