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
        assert_eq!(snap.phase, TpuPhase::Stopped);
        assert!(marker.exists(), "launch.py stop was not invoked");
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
}
