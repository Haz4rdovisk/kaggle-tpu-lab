#!/usr/bin/env python3
import sys

def sub(path, old, new):
    s = open(path, encoding="utf-8").read()
    assert old in s, (path, old[:80])
    s = s.replace(old, new)
    open(path, "w", encoding="utf-8", newline="\n").write(s)
    print(path, "ok")

# 1. testutil import path
sub("src/state/mod.rs",
    "use crate::kaggle::FakeKaggleApi;",
    "use crate::kaggle::testutil::FakeKaggleApi;")

# 2. Debug on SessionSnapshot
sub("src/state/mod.rs",
    "#[derive(Clone, serde::Serialize)]\npub struct SessionSnapshot {",
    "#[derive(Clone, Debug, serde::Serialize)]\npub struct SessionSnapshot {")

# 3. temp array lifetime in stop test
venv_py = r'C:\Users\Pc_Lu\.Dev-projects\kaggle-tpu-lab\.venv\Scripts\python.exe'
sub("src/kaggle/mod.rs",
    "let python = [\n" +
    '            std::path::PathBuf::from(r"%s"),\n' % venv_py +
    '            std::path::PathBuf::from("python3"),\n' +
    '            std::path::PathBuf::from("python"),\n' +
    "        ]\n" +
    "        .iter()\n" +
    "        .find(|p| p.exists());",
    "let candidates = [\n" +
    '            std::path::PathBuf::from(r"%s"),\n' % venv_py +
    '            std::path::PathBuf::from("python3"),\n' +
    '            std::path::PathBuf::from("python"),\n' +
    "        ];\n" +
    "        let python = candidates.iter().find(|p| p.exists());")

# 4. cfg(test) testutil
sub("src/kaggle/mod.rs",
    "/// Fakes for unit tests (compiled in all targets so cross-module tests use\n/// them too).\npub mod testutil {",
    "/// Fakes for unit tests (available across modules under `cargo test`).\n#[cfg(test)]\npub mod testutil {")

# 5. use TRAY_STOPPED for Stopped
sub("src/tray.rs",
    "        TpuPhase::Idle | TpuPhase::Stopped => TRAY_IDLE,",
    "        TpuPhase::Idle => TRAY_IDLE,\n        TpuPhase::Stopped => TRAY_STOPPED,")

# 6. remove unused Step enum
sub("src/state/machine.rs",
    "#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum Step {\n    Runtime,\n    Cache,\n    Weights,\n    Server,\n    Tunnel,\n    Ready,\n}\n\n",
    "")

# 7. pi_sync_error wiring
sub("src/state/mod.rs",
    "    /// Channel used to request an immediate tick (panel open / tray action).\n    pub refresh_tx: Mutex<Option<std::sync::mpsc::Sender<()>>>,\n}",
    "    /// Channel used to request an immediate tick (panel open / tray action).\n    pub refresh_tx: Mutex<Option<std::sync::mpsc::Sender<()>>>,\n    /// Last Pi sync failure (UI shows SYNC FAILED + Retry until next success).\n    pub pi_sync_error: Mutex<Option<String>>,\n}")
sub("src/state/mod.rs",
    "            last_snapshot_json: Mutex::new(String::new()),\n            refresh_tx: Mutex::new(None),\n        }",
    "            last_snapshot_json: Mutex::new(String::new()),\n            refresh_tx: Mutex::new(None),\n            pi_sync_error: Mutex::new(None),\n        }")
sub("src/state/mod.rs",
    "        let pi = crate::pi::assess_current(m.endpoint.as_deref());",
    "        let pi = if self.pi_sync_error.lock().unwrap().is_some() {\n            crate::pi::PiState::SyncFailed\n        } else {\n            crate::pi::assess_current(m.endpoint.as_deref())\n        };")
sub("src/kaggle/mod.rs",
    "pub fn sync_pi(app: &AppHandle) -> Result<String, String> {\n    let st = app.state::<AppState>();\n    let msg = sync_pi_state(&st)?;\n    emit_snapshot(app, &st, &st.snapshot(now_secs()));\n    Ok(msg)\n}",
    "pub fn sync_pi(app: &AppHandle) -> Result<String, String> {\n    let st = app.state::<AppState>();\n    match sync_pi_state(&st) {\n        Ok(msg) => {\n            *st.pi_sync_error.lock().unwrap() = None;\n            emit_snapshot(app, &st, &st.snapshot(now_secs()));\n            Ok(msg)\n        }\n        Err(e) => {\n            *st.pi_sync_error.lock().unwrap() = Some(e.clone());\n            emit_snapshot(app, &st, &st.snapshot(now_secs()));\n            Err(e)\n        }\n    }\n}")

print("all patches applied")
