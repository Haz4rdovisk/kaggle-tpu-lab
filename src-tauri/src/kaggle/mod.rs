//! Orchestration: polling tick, session recovery, start (with double-start
//! prevention) and stop.
//!
//! All session logic lives in pure functions over [`AppState`] (no Tauri
//! dependency) so it is fully unit-testable. The thin `*` wrappers add the
//! Tauri side effects (emit to the panel, tray updates, panel reveal).

pub mod events;
pub mod http;
pub mod launcher;
pub mod ntfy;
pub mod probe;

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::state::machine::{KaggleStatus, MachineState, NoteKind, TpuPhase};
use crate::state::{now_secs, AppState, SessionSnapshot};
use launcher::LocalState;
use probe::EndpointProbe;

pub use ntfy::{EventSource, NtfySource};
pub use probe::HttpProbe;

/// Kaggle kernel status access (CLI in production, fakes in tests).
pub trait KaggleApi: Send + Sync {
    fn kernel_status(&self, kernel: &str) -> KaggleStatus;
}

pub struct CliKaggleApi(pub launcher::Launcher);

impl KaggleApi for CliKaggleApi {
    fn kernel_status(&self, kernel: &str) -> KaggleStatus {
        self.0.kernel_status(kernel)
    }
}

/// Fakes for unit tests (available across modules under `cargo test`).
#[cfg(test)]
pub mod testutil {
    use super::*;

    #[derive(Clone)]
    pub struct FakeKaggleApi {
        pub status: KaggleStatus,
    }

    impl FakeKaggleApi {
        pub fn queued() -> Self {
            Self { status: KaggleStatus::Queued }
        }
        pub fn running() -> Self {
            Self { status: KaggleStatus::Running }
        }
        pub fn complete() -> Self {
            Self { status: KaggleStatus::Complete }
        }
        pub fn not_found() -> Self {
            Self { status: KaggleStatus::NotFound }
        }
        pub fn unavailable() -> Self {
            Self { status: KaggleStatus::Unavailable }
        }
    }

    impl KaggleApi for FakeKaggleApi {
        fn kernel_status(&self, _kernel: &str) -> KaggleStatus {
            self.status
        }
    }

    pub struct FakeEvents {
        pub events: Vec<crate::state::machine::LauncherEvent>,
        /// Runtime-toggleable failure flag (simulates an ntfy outage
        /// mid-session without rebuilding the harness).
        pub fail: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl std::fmt::Debug for FakeEvents {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("FakeEvents").finish()
        }
    }

    impl FakeEvents {
        pub fn new(events: Vec<crate::state::machine::LauncherEvent>, fail: bool) -> Self {
            Self {
                events,
                fail: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(fail)),
            }
        }
    }

    impl EventSource for FakeEvents {
        fn fetch(&self, _topic: &str, _since: i64) -> Result<Vec<crate::state::machine::LauncherEvent>, String> {
            if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
                Err("fake ntfy unreachable".into())
            } else {
                Ok(self.events.clone())
            }
        }
    }

    #[derive(Clone, Copy)]
    pub struct FakeProbe {
        pub ok: bool,
    }

    impl EndpointProbe for FakeProbe {
        fn probe(&self, _endpoint: &str, _api_key: &str) -> bool {
            self.ok
        }
    }
}

const PROBE_WHILE_UNKNOWN_EVERY: i64 = 120; // s
const PROBE_WHILE_READY_EVERY: i64 = 300; // s
const MAX_PROBE_FAILS: u32 = 3;
const STALE_UNVERIFIED_STATE_SECS: i64 = 24 * 60 * 60;

/// One polling iteration over the raw state (no Tauri side effects):
/// state file -> Kaggle status -> ntfy events -> reconcile -> probe.
/// Never fails: bad sources are tolerated by design.
pub fn tick_state(st: &AppState) -> SessionSnapshot {
    let now = now_secs();

    // 1. local state file (kernel/topic/key presence)
    let local = LocalState::read(&st.state_file);
    {
        let mut key = st.api_key.lock().unwrap();
        *key = local.as_ref().and_then(|_| LocalState::raw_api_key(&st.state_file));
    }
    {
        let mut m = st.machine.lock().unwrap();
        match &local {
            Some(l) => {
                if m.kernel.as_deref() != Some(l.kernel.as_str()) {
                    // Boot/reattach path: never fabricate a queue result.
                    // QUEUED is only entered after Kaggle reports it.
                    m.reattach_model(&l.kernel, Some(l.topic.clone()), l.model);
                    m.push_note(now, "Reattached to existing session", NoteKind::Info);
                }
            }
            None => {
                if m.kernel.is_some() {
                    *m = MachineState::default();
                }
            }
        }
    }

    // 2. Kaggle kernel status (authoritative for the coarse lifecycle)
    let kstatus = {
        let m = st.machine.lock().unwrap();
        match m.kernel.clone() {
            Some(k) => st.kaggle_api.kernel_status(&k),
            None => KaggleStatus::NotFound,
        }
    };
    *st.last_kaggle_status.lock().unwrap() = Some(kstatus);

    // 3. structured events (auxiliary; failures are tolerated)
    {
        let mut m = st.machine.lock().unwrap();
        if let Some(topic) = m.topic.clone().filter(|t| !t.is_empty()) {
            let since = if m.events_applied {
                (m.last_event_ts.saturating_sub(5)).max(now - 86_400)
            } else {
                now - 7 * 86_400
            };
            match st.event_source.fetch(&topic, since) {
                Ok(mut evs) => {
                    evs.sort_by_key(|e| e.ts);
                    let fresh = evs
                        .into_iter()
                        .filter(|e| !m.events_applied || e.ts > m.last_event_ts)
                        .collect::<Vec<_>>();
                    for e in fresh {
                        m.apply_event(&e);
                    }
                    m.events_applied = true;
                    m.ntfy_reachable = true;
                }
                Err(_) => {
                    // ntfy down: keep everything as-is; never mark dead.
                    m.ntfy_reachable = false;
                }
            }
        }
    }

    // A private-kernel API bug can leave a successful-looking local push
    // permanently unverifiable. Only discard it when it is very old, ntfy is
    // reachable, and there is no launcher evidence at all. This is a safety
    // valve, not a normal lifecycle transition.
    let expire_unverified = if kstatus == KaggleStatus::Unavailable {
        match &local {
            Some(l) => {
                let m = st.machine.lock().unwrap();
                m.events_applied
                    && m.ntfy_reachable
                    && m.last_event_ts == 0
                    && m.endpoint.is_none()
                    && l
                        .age_secs(&st.state_file, now)
                        .map(|age| age >= STALE_UNVERIFIED_STATE_SECS)
                        .unwrap_or(false)
            }
            None => false,
        }
    } else {
        false
    };
    if expire_unverified {
        let same_state = local.as_ref().is_some_and(|old| {
            LocalState::read(&st.state_file)
                .map(|cur| cur.kernel == old.kernel && cur.topic == old.topic)
                .unwrap_or(false)
        });
        if same_state {
            let _ = std::fs::remove_file(&st.state_file);
            *st.api_key.lock().unwrap() = None;
            *st.last_kaggle_status.lock().unwrap() = None;
            let mut m = st.machine.lock().unwrap();
            *m = MachineState::default();
            m.push_note(
                now,
                "Discarded stale unverified local session state",
                NoteKind::Warn,
            );
            drop(m);
            return st.snapshot(now);
        }
    }

    // 4. reconcile with the coarse status
    {
        let mut m = st.machine.lock().unwrap();
        m.reconcile(kstatus, now);
    }

    // 5. endpoint probe: ground truth when events are absent or stale
    {
        let (do_probe, mode) = {
            let m = st.machine.lock().unwrap();
            let has_key = st.api_key.lock().unwrap().is_some();
            if m.endpoint.is_none() || !has_key || m.terminal.is_some() {
                (false, 0)
            } else if m.phase == TpuPhase::Ready {
                let due = now - st.last_probe_at.load(Ordering::Relaxed) >= PROBE_WHILE_READY_EVERY;
                (due, 1)
            } else if matches!(
                kstatus,
                KaggleStatus::Running | KaggleStatus::Unknown | KaggleStatus::Unavailable
            ) {
                let due = now - st.last_probe_at.load(Ordering::Relaxed) >= PROBE_WHILE_UNKNOWN_EVERY;
                (due, 2)
            } else {
                (false, 0)
            }
        };
        if do_probe {
            let (endpoint, key) = {
                let m = st.machine.lock().unwrap();
                (
                    m.endpoint.clone().unwrap(),
                    st.api_key.lock().unwrap().clone().unwrap(),
                )
            };
            let ok = st.endpoint_probe.probe(&endpoint, &key);
            st.last_probe_at.store(now, Ordering::Relaxed);
            let mut m = st.machine.lock().unwrap();
            if m.terminal.is_none() {
                if mode == 1 {
                    // liveness check on a READY session
                    if ok {
                        m.probe_fails = 0;
                    } else {
                        m.probe_fails += 1;
                        if m.probe_fails >= MAX_PROBE_FAILS {
                            m.phase = TpuPhase::Failed;
                            m.terminal = Some("endpoint-unreachable".into());
                            m.error =
                                Some("Endpoint unreachable (3 consecutive probe failures)".into());
                            m.push_note(now, "Endpoint unreachable", NoteKind::Error);
                        }
                    }
                } else {
                    // recovery: no READY event visible (expired/absent ntfy)
                    if ok {
                        if m.phase != TpuPhase::Ready {
                            m.phase = TpuPhase::Ready;
                            m.endpoint_live = true;
                            m.ready_at = Some(now);
                            m.ready_at_estimated = true;
                            m.keepalive_min
                                .get_or_insert(st.settings.lock().unwrap().keepalive_min);
                            m.serving_from = Some(now);
                            m.serving_from_estimated = true;
                            m.probe_fails = 0;
                            m.push_note(
                                now,
                                "Endpoint verified live (direct probe)",
                                NoteKind::Success,
                            );
                        } else {
                            m.probe_fails = 0;
                        }
                    }
                }
            }
        }
    }

    st.snapshot(now)
}

/// Start (or reattach). Never creates a second session while one is
/// QUEUED or RUNNING.
pub fn start_state(st: &AppState) -> Result<SessionSnapshot, String> {
    if st
        .starting
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("A start is already in progress".into());
    }
    let result = do_start_state(st);
    st.starting.store(false, Ordering::SeqCst);
    result
}

fn do_start_state(st: &AppState) -> Result<SessionSnapshot, String> {
    let now = now_secs();
    let local = LocalState::read(&st.state_file);

    if let Some(l) = &local {
        // Consult Kaggle BEFORE deciding anything (double-start guard).
        let status = st.kaggle_api.kernel_status(&l.kernel);
        if matches!(
            status,
            KaggleStatus::Queued
                | KaggleStatus::Running
                | KaggleStatus::Unknown
                | KaggleStatus::Unavailable
        ) {
            {
                let mut m = st.machine.lock().unwrap();
                if m.kernel.as_deref() != Some(l.kernel.as_str()) {
                    m.reset_for_kernel_model(&l.kernel, Some(l.topic.clone()), l.model);
                    m.push_note(
                        now,
                        "Reattached to existing session",
                        NoteKind::Info,
                    );
                }
            }
            {
                let mut key = st.api_key.lock().unwrap();
                *key = LocalState::raw_api_key(&st.state_file);
            }
            return Ok(tick_state(st));
        }
        // Only a terminal/definitively missing kernel may fall through to a
        // fresh push. Unknown is deliberately guarded above to avoid creating
        // a second TPU while Kaggle's private-kernel API is unavailable.
    }

    // Fresh push through the existing launcher flow.
    let args = {
        let s = st.settings.lock().unwrap();
        s.validate()?;
        s.serve_args()
    };
    let mut child = st.launcher.serve(&args).map_err(|e| {
        format!(
            "Failed to start the launcher: {}",
            crate::security::redact_for_log(&e.to_string())
        )
    })?;

    let old_topic = local.as_ref().map(|l| l.topic.clone()).unwrap_or_default();

    // Wait for launch.py to write the new state file (push confirmed), then
    // detach from its watch loop (Ctrl-C-equivalent; the kernel keeps running
    // server-side).
    let mut outcome: Result<SessionSnapshot, String> =
        Err("Timed out waiting for Kaggle to accept the kernel push".into());
    for _ in 0..90 {
        std::thread::sleep(Duration::from_secs(2));
        let cur = LocalState::read(&st.state_file);
        if let Some(l) = cur {
            if !l.topic.is_empty() && l.topic != old_topic {
                let _ = child.kill();
                {
                    let mut m = st.machine.lock().unwrap();
                    m.reset_for_kernel_model(&l.kernel, Some(l.topic.clone()), l.model);
                    m.push_note(now, "Kernel submitted", NoteKind::Info);
                }
                {
                    let mut key = st.api_key.lock().unwrap();
                    *key = LocalState::raw_api_key(&st.state_file);
                }
                outcome = Ok(tick_state(st));
                break;
            }
        }
        if let Ok(Some(_status)) = child.try_wait() {
            let _ = child.wait();
            outcome = Err(
                "The launcher exited before Kaggle confirmed the push. Check Kaggle auth and quota, then retry."
                    .into(),
            );
            break;
        }
    }
    if outcome.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    outcome
}

/// Official stop through launch.py. A successful stop removes the launcher
/// state file and resets the in-memory session immediately so a subsequent
/// Start cannot reattach to a dead kernel.
pub fn stop_state(st: &AppState) -> Result<SessionSnapshot, String> {
    let previous_phase = st.machine.lock().unwrap().phase;
    let can_stop = previous_phase.can_stop();
    if !can_stop {
        return Err("There is no active TPU session to stop".into());
    }
    if st
        .stopping
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("A stop is already in progress".into());
    }

    {
        let mut m = st.machine.lock().unwrap();
        if m.phase != TpuPhase::Stopping {
            m.phase = TpuPhase::Stopping;
            m.push_note(now_secs(), "Stopping session", NoteKind::Info);
        }
    }

    let stop_result = st.launcher.stop();
    st.stopping.store(false, Ordering::SeqCst);
    match &stop_result {
        Ok(_out) => {
            // launch.py already removes the file; keep this defensive cleanup
            // so a future launcher change cannot resurrect a stopped session.
            let _ = std::fs::remove_file(&st.state_file);
            *st.api_key.lock().unwrap() = None;
            *st.last_kaggle_status.lock().unwrap() = None;
            let mut m = st.machine.lock().unwrap();
            *m = MachineState::default();
            m.push_note(now_secs(), "TPU session stopped", NoteKind::Info);
        }
        Err(msg) => {
            let mut m = st.machine.lock().unwrap();
            // Restore the exact phase from before the Stop attempt.
            m.phase = previous_phase;
            m.error = Some(crate::security::redact_for_log(msg));
            m.push_note(now_secs(), "Stop failed", NoteKind::Error);
        }
    }
    let snap = st.snapshot(now_secs());
    match stop_result {
        Ok(_) => Ok(snap),
        Err(msg) => Err(msg),
    }
}

// ---------------------------------------------------------------------------
// Tauri wrappers (side effects only)
// ---------------------------------------------------------------------------

fn emit_snapshot(app: &AppHandle, st: &AppState, snap: &SessionSnapshot) {
    let json = serde_json::to_string(snap).unwrap_or_default();
    let changed = {
        let mut last = st.last_snapshot_json.lock().unwrap();
        let ch = *last != json;
        *last = json;
        ch
    };
    if changed {
        if let Err(e) = app.emit("session-update", snap) {
            let _ = e; // panel may not exist yet
        }
        crate::tray::update_tray(app, snap);
    }
}

/// One polling iteration + side effects.
pub fn tick(app: &AppHandle) -> SessionSnapshot {
    let st = app.state::<AppState>();
    // If another tick is already running (manual refresh concurrent with the
    // poller), return the current state without duplicating network I/O.
    if st.ticking.swap(true, Ordering::SeqCst) {
        return st.snapshot(now_secs());
    }
    let snap = tick_state(&st);
    emit_snapshot(app, &st, &snap);
    st.ticking.store(false, Ordering::SeqCst);
    #[cfg(debug_assertions)]
    if std::env::var_os("KAGGLE_KTL_DEBUG").is_some() {
        let viewport = app
            .get_webview_window("main")
            .and_then(|w| match (w.inner_size(), w.scale_factor()) {
                (Ok(sz), Ok(sf)) if sf > 0.0 => Some(format!(
                    "viewport(physical)={:?}x{:?} scale={:.2} css={:.0}x{:.0}",
                    sz.width, sz.height, sf, sz.width as f64 / sf, sz.height as f64 / sf
                )),
                _ => None,
            })
            .unwrap_or_else(|| "viewport=?".to_string());
        eprintln!(
            "[tick] {} phase={:?} kernel={:?} endpoint={:?} live={} uptime={:?} ntfy={} err={:?}",
            viewport, snap.phase, snap.kernel, snap.endpoint, snap.endpoint_live,
            snap.uptime_secs, snap.ntfy_reachable, snap.error
        );
    }
    snap
}

pub fn start_session(app: &AppHandle) -> Result<SessionSnapshot, String> {
    let st = app.state::<AppState>();
    let snap = start_state(&st)?;
    emit_snapshot(app, &st, &snap);
    Ok(snap)
}

pub fn stop_session(app: &AppHandle) -> Result<SessionSnapshot, String> {
    let st = app.state::<AppState>();
    let snap = stop_state(&st)?;
    emit_snapshot(app, &st, &snap);
    Ok(snap)
}

/// Single polling loop with phase-dependent intervals. One loop only; a
/// refresh request shortens the wait instead of spawning another loop.
pub fn start_poller(app: AppHandle) {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    {
        let st = app.state::<AppState>();
        *st.refresh_tx.lock().unwrap() = Some(tx);
    }
    std::thread::Builder::new()
        .name("kaggle-poller".into())
        .spawn(move || {
            loop {
                let snap = tick(&app);
                // Reveal the panel exactly once, after the first successful
                // tick. Never re-show or steal focus afterwards: the user
                // controls visibility via the tray (left-click opens) and the
                // close button (hides to tray).
                let st = app.state::<AppState>();
                if !st.primed.swap(true, Ordering::AcqRel) {
                    crate::tray::show_companion_panel(&app, None, false);
                }
                let interval = poll_interval_secs(snap.phase);
                let deadline = now_secs() + interval;
                loop {
                    let now = now_secs();
                    let remaining = deadline - now;
                    if remaining <= 0 {
                        break;
                    }
                    let _ = rx.recv_timeout(Duration::from_secs(remaining.clamp(0, 5) as u64));
                }
            }
        })
        .expect("failed to spawn poller thread");
}

/// Immediate refresh request (panel focus / tray actions).
pub fn request_refresh(app: &AppHandle) {
    let st = app.state::<AppState>();
    let tx = st.refresh_tx.lock().unwrap().clone();
    if let Some(tx) = tx {
        let _ = tx.send(());
    }
}

pub fn poll_interval_secs(phase: TpuPhase) -> i64 {
    match phase {
        TpuPhase::Idle | TpuPhase::Stopped | TpuPhase::Failed => 60,
        TpuPhase::Verifying | TpuPhase::Queued => 15,
        TpuPhase::Provisioning
        | TpuPhase::Starting
        | TpuPhase::LoadingWeights
        | TpuPhase::Compiling => 12,
        TpuPhase::Healthy => 15,
        TpuPhase::Ready => 30,
        TpuPhase::Stopping => 5,
    }
}

// ---------------------------------------------------------------------------
// Tests — hermetic: fakes for Kaggle API, ntfy and probe; temp state files.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kaggle::testutil::{FakeEvents, FakeKaggleApi, FakeProbe};
    use crate::state::machine::LauncherEvent;
    use std::path::PathBuf;

    fn make_state(
        kaggle: FakeKaggleApi,
        events: FakeEvents,
        probe: FakeProbe,
        state: Option<serde_json::Value>,
    ) -> (AppState, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "ktl-harness-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let state_file = dir.join("state.json");
        match state {
            Some(v) => std::fs::write(&state_file, serde_json::to_string(&v).unwrap()).unwrap(),
            None => {
                let _ = std::fs::remove_file(&state_file);
            }
        }
        let launcher = launcher::Launcher {
            python: dir.join("python.exe"),
            project_root: dir.clone(),
        };
        let st = AppState::new(
            launcher,
            state_file.clone(),
            Box::new(kaggle),
            Box::new(events),
            Box::new(probe),
        );
        (st, state_file)
    }

    fn state_val(kernel: &str, topic: &str, key: &str) -> serde_json::Value {
        serde_json::json!({"kernel": kernel, "topic": topic, "api_key": key})
    }

    fn ev(ts: i64, phase: &str, fields: serde_json::Value) -> LauncherEvent {
        LauncherEvent {
            ts,
            phase: phase.into(),
            fields,
        }
    }

    fn ready_ev(ts: i64) -> LauncherEvent {
        ev(
            ts,
            "ready",
            serde_json::json!({"endpoint": "https://apollo.trycloudflare.com/v1", "keepalive_min": 480, "max_model_len": 262144}),
        )
    }

    fn tunnel_ev(ts: i64) -> LauncherEvent {
        ev(
            ts,
            "tunnel-url",
            serde_json::json!({"endpoint": "https://apollo.trycloudflare.com/v1"}),
        )
    }

    #[test]
    fn recovery_finds_ready_session() {
        let (st, _) = make_state(
            FakeKaggleApi::running(),
            FakeEvents::new(
                vec![
                    ev(1_000, "server-launch", serde_json::json!({"max_model_len": 262144, "mtp": 3, "text_only": true})),
                    tunnel_ev(1_100),
                    ready_ev(2_000),
                    ev(2_500, "benchmark", serde_json::json!({"decode_tok_s": 106.8})),
                ],
                false,
            ),
            FakeProbe { ok: true },
            Some(state_val("u/k", "ktl-topic", "sk-testkey")),
        );
        let snap = tick_state(&st);
        assert_eq!(snap.phase, TpuPhase::Ready);
        assert!(snap.endpoint_live);
        assert_eq!(snap.endpoint.as_deref(), Some("https://apollo.trycloudflare.com/v1"));
        assert!(snap.has_api_key);
        assert_eq!(snap.decode_tok_s, Some(106.8));
        let json = serde_json::to_string(&snap).unwrap();
        assert!(!json.contains("sk-testkey"), "snapshot leaks the key");
    }

    #[test]
    fn ntfy_outage_keeps_ready_session_live() {
        // Phase 1: ntfy healthy — session recovers to READY.
        let fail = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (st, _) = make_state(
            FakeKaggleApi::running(),
            FakeEvents {
                events: vec![ready_ev(2_000)],
                fail: fail.clone(),
            },
            FakeProbe { ok: true },
            Some(state_val("u/k", "ktl-topic", "sk-testkey")),
        );
        let snap = tick_state(&st);
        assert_eq!(snap.phase, TpuPhase::Ready);
        assert!(snap.ntfy_reachable);

        // Phase 2: ntfy goes down. The session must stay READY (last known
        // good state) and report ntfy as unreachable — never Failed.
        fail.store(true, std::sync::atomic::Ordering::SeqCst);
        let snap = tick_state(&st);
        assert_eq!(snap.phase, TpuPhase::Ready);
        assert!(snap.endpoint_live);
        assert!(!snap.ntfy_reachable);
        assert!(snap.error.is_none());
    }

    #[test]
    fn recovery_without_ready_event_uses_probe() {
        // ntfy history knows the reserved endpoint but the READY event is
        // missing (expired or never published); Kaggle says RUNNING and the
        // direct probe confirms liveness -> Ready, flagged estimated.
        let (st, _) = make_state(
            FakeKaggleApi::running(),
            FakeEvents::new(vec![tunnel_ev(1_000)], false),
            FakeProbe { ok: true },
            Some(state_val("u/k", "ktl-topic", "sk-testkey")),
        );
        let snap = tick_state(&st);
        assert_eq!(snap.phase, TpuPhase::Ready);
        assert!(snap.ready_at_estimated);
        assert!(snap.ntfy_reachable);
    }

    #[test]
    fn ntfy_failure_alone_never_kills_running_session() {
        let (st, _) = make_state(
            FakeKaggleApi::running(),
            FakeEvents::new(vec![], true),
            FakeProbe { ok: false },
            Some(state_val("u/k", "ktl-topic", "sk-testkey")),
        );
        // No events at all (ntfy down) and probe unavailable: the session
        // stays active (Provisioning), never Failed.
        let snap = tick_state(&st);
        assert_ne!(snap.phase, TpuPhase::Failed);
        assert!(matches!(snap.phase, TpuPhase::Provisioning | TpuPhase::Queued));
    }

    #[test]
    fn repeated_probe_failures_fail_the_session() {
        let (st, _) = make_state(
            FakeKaggleApi::running(),
            FakeEvents::new(vec![ready_ev(2_000)], false),
            FakeProbe { ok: false },
            Some(state_val("u/k", "ktl-topic", "sk-testkey")),
        );
        // Force liveness probes (mode 1) by making the deadline always due.
        st.last_probe_at.store(0, Ordering::SeqCst);
        tick_state(&st); // probe fail 1
        st.last_probe_at.store(0, Ordering::SeqCst);
        tick_state(&st); // probe fail 2
        st.last_probe_at.store(0, Ordering::SeqCst);
        let snap = tick_state(&st); // probe fail 3 -> Failed
        assert_eq!(snap.phase, TpuPhase::Failed);
        assert!(snap.error.as_deref().unwrap().contains("unreachable"));
    }

    #[test]
    fn double_start_is_prevented() {
        // Existing QUEUED session: start must reattach, never push.
        let (st, _) = make_state(
            FakeKaggleApi::queued(),
            FakeEvents::new(vec![], false),
            FakeProbe { ok: false },
            Some(state_val("u/k", "ktl-topic", "sk-testkey")),
        );
        let snap = start_state(&st).expect("reattach must succeed");
        assert_eq!(snap.phase, TpuPhase::Queued);
        assert_eq!(snap.kernel.as_deref(), Some("u/k"));

        // Existing RUNNING + READY session: same guard.
        let (st2, _) = make_state(
            FakeKaggleApi::running(),
            FakeEvents::new(vec![ready_ev(2_000)], false),
            FakeProbe { ok: true },
            Some(state_val("u/k", "ktl-topic", "sk-testkey")),
        );
        let snap2 = start_state(&st2).expect("reattach");
        assert_eq!(snap2.phase, TpuPhase::Ready);
        assert_eq!(snap2.kernel.as_deref(), Some("u/k"));
    }

    #[test]
    fn unavailable_private_kernel_does_not_allow_double_start() {
        let (st, _) = make_state(
            FakeKaggleApi::unavailable(),
            FakeEvents::new(vec![], false),
            FakeProbe { ok: false },
            Some(state_val("u/k", "ktl-topic", "sk-testkey")),
        );
        let snap = start_state(&st).expect("must reattach while status is unavailable");
        assert_eq!(snap.phase, TpuPhase::Verifying);
        assert_eq!(snap.kernel.as_deref(), Some("u/k"));
        assert!(!snap.phase.can_start());
    }

    #[test]
    fn stale_unverified_state_is_discarded_when_ntfy_confirms_no_activity() {
        let stale = serde_json::json!({
            "kernel": "u/k",
            "topic": "ktl-topic",
            "api_key": "sk-testkey",
            "model": "glm53-flash",
            "submitted_at": 1
        });
        let (st, state_file) = make_state(
            FakeKaggleApi::unavailable(),
            FakeEvents::new(vec![], false),
            FakeProbe { ok: false },
            Some(stale),
        );
        let snap = tick_state(&st);
        assert_eq!(snap.phase, TpuPhase::Idle);
        assert_eq!(snap.kernel, None);
        assert!(!snap.has_api_key);
        assert!(!state_file.exists());
        assert!(snap
            .activity
            .iter()
            .any(|n| n.text.contains("Discarded stale unverified")));
    }

    #[test]
    fn recent_unverified_state_is_preserved() {
        let recent = serde_json::json!({
            "kernel": "u/k",
            "topic": "ktl-topic",
            "api_key": "sk-testkey",
            "model": "glm53-flash",
            "submitted_at": now_secs()
        });
        let (st, state_file) = make_state(
            FakeKaggleApi::unavailable(),
            FakeEvents::new(vec![], false),
            FakeProbe { ok: false },
            Some(recent),
        );
        let snap = tick_state(&st);
        assert_eq!(snap.phase, TpuPhase::Verifying);
        assert!(state_file.exists());
    }

    #[test]
    fn stale_unverified_state_is_preserved_if_ntfy_is_unreachable() {
        let stale = serde_json::json!({
            "kernel": "u/k",
            "topic": "ktl-topic",
            "api_key": "sk-testkey",
            "model": "glm53-flash",
            "submitted_at": 1
        });
        let (st, state_file) = make_state(
            FakeKaggleApi::unavailable(),
            FakeEvents::new(vec![], true),
            FakeProbe { ok: false },
            Some(stale),
        );
        let snap = tick_state(&st);
        assert_eq!(snap.phase, TpuPhase::Verifying);
        assert!(state_file.exists());
    }

    #[test]
    fn stop_requires_active_session() {
        let (st, _) = make_state(
            FakeKaggleApi::not_found(),
            FakeEvents::new(vec![], false),
            FakeProbe { ok: false },
            None,
        );
        tick_state(&st);
        let res = stop_state(&st);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("no active TPU session"));
    }

    #[test]
    fn stop_with_active_session_runs_launcher_and_stops() {
        // Fake launcher: a temp dir with a stub launch.py that records the
        // subcommand and exits 0. The "python" is any usable interpreter.
        let candidates = [
            PathBuf::from(r"C:\Users\Pc_Lu\.Dev-projects\kaggle-tpu-lab\.venv\Scripts\python.exe"),
            PathBuf::from("python3"),
            PathBuf::from("python"),
        ];
        let Some(python) = candidates.iter().find(|p| p.exists()) else {
            eprintln!("skip: no python interpreter found");
            return;
        };

        let dir = std::env::temp_dir().join(format!(
            "ktl-stop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("stop-called.txt");
        let stub = format!(
            "import sys\nif 'stop' in sys.argv:\n    open(r'{}', 'w').write('stopped')\nprint('done')\n",
            marker.display().to_string().replace('\\', "\\\\")
        );
        std::fs::write(dir.join("launch.py"), stub).unwrap();

        let state_file = dir.join("state.json");
        std::fs::write(
            &state_file,
            serde_json::to_string(&state_val("u/k", "ktl-topic", "sk-x")).unwrap(),
        )
        .unwrap();
        let st = AppState::new(
            launcher::Launcher {
                python: python.clone(),
                project_root: dir.clone(),
            },
            state_file,
            Box::new(FakeKaggleApi::running()),
            Box::new(FakeEvents::new(vec![ready_ev(2_000)], false)),
            Box::new(FakeProbe { ok: true }),
        );
        tick_state(&st); // Ready
        let snap = stop_state(&st).expect("stop must succeed");
        assert_eq!(snap.phase, TpuPhase::Idle);
        assert_eq!(snap.kernel, None);
        assert!(!snap.has_api_key);
        assert!(snap.phase.can_start());
        assert!(!st.state_file.exists(), "successful stop must clear launcher state");
        assert!(marker.exists(), "launch.py stop was not invoked");

        st.settings.lock().unwrap().model = crate::state::model::ModelId::Glm53Flash;
        let after_model_switch = st.snapshot(now_secs());
        assert_eq!(after_model_switch.model, Some(crate::state::model::ModelId::Glm53Flash));
        assert!(after_model_switch.phase.can_start());
        assert_eq!(after_model_switch.kernel, None);
    }

    #[test]
    fn failed_stop_preserves_session_and_state_file() {
        let candidates = [
            PathBuf::from(r"C:\Users\Pc_Lu\.Dev-projects\kaggle-tpu-lab\.venv\Scripts\python.exe"),
            PathBuf::from("python3"),
            PathBuf::from("python"),
        ];
        let Some(python) = candidates.iter().find(|p| p.exists()) else {
            eprintln!("skip: no python interpreter found");
            return;
        };

        let dir = std::env::temp_dir().join(format!(
            "ktl-stop-fail-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("launch.py"),
            "import sys\nprint('delete failed')\nsys.exit(1)\n",
        )
        .unwrap();

        let state_file = dir.join("state.json");
        std::fs::write(
            &state_file,
            serde_json::to_string(&state_val("u/k", "ktl-topic", "sk-x")).unwrap(),
        )
        .unwrap();
        let st = AppState::new(
            launcher::Launcher {
                python: python.clone(),
                project_root: dir,
            },
            state_file.clone(),
            Box::new(FakeKaggleApi::running()),
            Box::new(FakeEvents::new(vec![ready_ev(2_000)], false)),
            Box::new(FakeProbe { ok: true }),
        );
        let before = tick_state(&st);
        assert_eq!(before.phase, TpuPhase::Ready);

        let err = stop_state(&st).expect_err("failed launcher stop must surface");
        assert!(err.contains("stop exited"));
        assert!(state_file.exists(), "failed stop must preserve launcher state");
        let after = st.snapshot(now_secs());
        assert_eq!(after.phase, TpuPhase::Ready);
        assert_eq!(after.kernel.as_deref(), Some("u/k"));
        assert!(after.has_api_key);
    }

    #[test]
    fn stale_state_file_settles_to_stopped() {
        let (st, _) = make_state(
            FakeKaggleApi::not_found(),
            FakeEvents::new(vec![], false),
            FakeProbe { ok: false },
            Some(state_val("u/old", "ktl-old", "sk-x")),
        );
        let snap = tick_state(&st);
        assert_eq!(snap.phase, TpuPhase::Stopped);
        assert!(snap.phase.can_start());
    }

    #[test]
    fn boot_reattach_does_not_fabricate_submission() {
        // Residual state file + kernel gone server-side: boot must reattach
        // (Idle) and settle to Stopped — never claim a submission.
        let (st, _) = make_state(
            FakeKaggleApi::not_found(),
            FakeEvents::new(vec![], false),
            FakeProbe { ok: false },
            Some(state_val("u/k", "ktl-topic", "sk-x")),
        );
        let snap = tick_state(&st);
        assert!(
            !snap.activity.iter().any(|n| n.text == "Kernel submitted"),
            "boot fabricated a submission: {:?}",
            snap.activity.iter().map(|n| &n.text).collect::<Vec<_>>()
        );
        assert_eq!(snap.phase, TpuPhase::Stopped);
    }

    #[test]
    fn queued_kernel_is_not_an_error() {
        let (st, _) = make_state(
            FakeKaggleApi::queued(),
            FakeEvents::new(vec![], false),
            FakeProbe { ok: false },
            Some(state_val("u/k", "ktl-topic", "sk-x")),
        );
        let snap = tick_state(&st);
        assert_eq!(snap.phase, TpuPhase::Queued);
        assert!(snap.error.is_none());
        assert!(snap.queued_at.is_some());
    }

    /// LIVE read-only check against the real Kaggle session. Never part of the
    /// default suite. Run explicitly:
    ///   cargo test --lib kaggle::tests::live_real_session_is_ready -- --ignored
    /// It only READS (kaggle status, ntfy, endpoint probe); it never stops,
    /// starts or creates anything.
    #[test]
    #[ignore]
    fn live_real_session_is_ready() {
        let launcher = launcher::Launcher::resolve(None).expect("launcher");
        let st = AppState::new(
            launcher,
            AppState::default_state_file(),
            Box::new(CliKaggleApi(
                launcher::Launcher::resolve(None).expect("launcher"),
            )),
            Box::new(NtfySource::default()),
            Box::new(HttpProbe::default()),
        );
        let snap = tick_state(&st);
        eprintln!(
            "LIVE phase={:?} kernel={:?} endpoint={:?} live={} uptime={:?} tok/s={:?} ntfy={} err={:?}",
            snap.phase, snap.kernel, snap.endpoint, snap.endpoint_live,
            snap.uptime_secs, snap.decode_tok_s, snap.ntfy_reachable, snap.error
        );
        // The real session must be detected as an active (not-dead) session.
        assert!(
            !matches!(snap.phase, TpuPhase::Failed),
            "real session flagged as failed: {:?}",
            snap.error
        );
        assert!(snap.has_api_key, "state file key not read");
    }
}
