#!/usr/bin/env python3
"""Patch kaggle/mod.rs: toggleable FakeEvents + corrected ntfy-outage tests."""

P = "src/kaggle/mod.rs"
s = open(P, encoding="utf-8").read()

def rep(old, new, tag):
    global s
    assert old in s, tag
    s = s.replace(old, new)
    print("ok:", tag)

# 1. FakeEvents struct with runtime-toggleable failure
rep("""    #[derive(Clone)]
    pub struct FakeEvents {
        pub events: Vec<crate::state::machine::LauncherEvent>,
        pub fail: bool,
    }

    impl EventSource for FakeEvents {
        fn fetch(&self, _topic: &str, _since: i64) -> Result<Vec<crate::state::machine::LauncherEvent>, String> {
            if self.fail {
                Err("fake ntfy unreachable".into())
            } else {
                Ok(self.events.clone())
            }
        }
    }""",
"""    pub struct FakeEvents {
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
    }""",
    "FakeEvents struct")

# 2. construction sites (explicit, one by one)
rep("""            FakeEvents {
                events: vec![
                    ev(1_000, "server-launch", serde_json::json!({"max_model_len": 262144, "mtp": 3, "text_only": true})),
                    ev(1_100, "tunnel-url", serde_json::json!({"endpoint": "https://apollo.trycloudflare.com/v1"})),
                    ready_ev(2_000),
                    ev(2_500, "benchmark", serde_json::json!({"decode_tok_s": 106.8})),
                ],
                fail: false,
            },""",
"""            FakeEvents::new(
                vec![
                    ev(1_000, "server-launch", serde_json::json!({"max_model_len": 262144, "mtp": 3, "text_only": true})),
                    ev(1_100, "tunnel-url", serde_json::json!({"endpoint": "https://apollo.trycloudflare.com/v1"})),
                    ready_ev(2_000),
                    ev(2_500, "benchmark", serde_json::json!({"decode_tok_s": 106.8})),
                ],
                false,
            ),""",
    "site recovery_finds_ready")

# The old broken test: replace it entirely with a two-phase outage test.
old_test_start = "    #[test]\n    fn recovery_ntfy_dead_running_kernel_uses_probe() {"
old_test_end = "    #[test]\n    fn ntfy_failure_alone_never_kills_running_session() {"
i0 = s.index(old_test_start)
i1 = s.index(old_test_end)
new_test = """    #[test]
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
            FakeEvents::new(
                vec![ev(
                    1_000,
                    "tunnel-url",
                    serde_json::json!({"endpoint": "https://apollo.trycloudflare.com/v1"}),
                )],
                false,
            ),
            FakeProbe { ok: true },
            Some(state_val("u/k", "ktl-topic", "sk-testkey")),
        );
        let snap = tick_state(&st);
        assert_eq!(snap.phase, TpuPhase::Ready);
        assert!(snap.ready_at_estimated);
        assert!(snap.ntfy_reachable);
    }

"""
s = s[:i0] + new_test + s[i1:]
print("ok: replaced broken test")

# 3. remaining simple sites (uniform shape)
import re
before = s.count("FakeEvents {")
s = s.replace("""            FakeEvents {
                events: vec![],
                fail: false,
            },""", """            FakeEvents::new(vec![], false),""")
s = s.replace("""            FakeEvents {
                events: vec![],
                fail: true,
            },""", """            FakeEvents::new(vec![], true),""")
s = s.replace("""            FakeEvents {
                events: vec![ready_ev(2_000)],
                fail: false,
            },""", """            FakeEvents::new(vec![ready_ev(2_000)], false),""")
s = s.replace("""            Box::new(FakeEvents {
                events: vec![ready_ev(2_000)],
                fail: false,
            }),""", """            Box::new(FakeEvents::new(vec![ready_ev(2_000)], false)),""")
after = s.count("FakeEvents {")
print("remaining struct-literal sites:", before, "->", after)
assert after <= 1, "unreplaced FakeEvents construction remains"

open(P, "w", encoding="utf-8", newline="\n").write(s)
print("all done")
