//! Pure state machine for the Kaggle TPU session.
//!
//! The machine is deterministic and side-effect free so every transition can be
//! unit-tested without touching Kaggle, ntfy or the filesystem.
//!
//! Sources of truth (in priority order):
//!   1. structured launcher events (ntfy topic) — fine-grained phases
//!   2. `kaggle kernels status` — QUEUED / RUNNING / COMPLETE / ERROR / CANCEL
//!   3. local state file — kernel id, topic, api key presence
//!
//! ntfy is auxiliary: if it is unreachable we keep the last known phase and let
//! the Kaggle status (or a direct endpoint probe) decide. We never mark the TPU
//! as dead just because ntfy is down.

use serde::{Deserialize, Serialize};

use crate::state::model::ModelId;

/// Explicit phase set. No free-form strings in the UI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TpuPhase {
    #[default]
    Idle,
    Verifying,
    Queued,
    Provisioning,
    Starting,
    LoadingWeights,
    Compiling,
    Healthy,
    Ready,
    Stopping,
    Stopped,
    Failed,
}

impl TpuPhase {
    /// Monotonic "progress" rank used to avoid backwards transitions.
    pub fn rank(self) -> u8 {
        match self {
            TpuPhase::Idle => 0,
            TpuPhase::Verifying => 1,
            TpuPhase::Queued => 2,
            TpuPhase::Provisioning => 3,
            TpuPhase::Starting => 4,
            TpuPhase::LoadingWeights => 5,
            TpuPhase::Compiling => 6,
            TpuPhase::Healthy => 7,
            TpuPhase::Ready => 8,
            TpuPhase::Stopping => 9,
            TpuPhase::Stopped => 9,
            TpuPhase::Failed => 10,
        }
    }

    pub fn is_active(self) -> bool {
        matches!(
            self,
            TpuPhase::Verifying
                | TpuPhase::Queued
                | TpuPhase::Provisioning
                | TpuPhase::Starting
                | TpuPhase::LoadingWeights
                | TpuPhase::Compiling
                | TpuPhase::Healthy
                | TpuPhase::Ready
        )
    }

    /// Phases in which a fresh Start is offered. Mirrors the frontend
    /// `CAN_START` set (`src/types/session.ts`): idle | stopped | failed.
    /// A failed session must be restartable — `start_state` resets the
    /// terminal flag via `reset_for_kernel` before pushing a new kernel.
    pub fn can_start(self) -> bool {
        matches!(
            self,
            TpuPhase::Idle | TpuPhase::Stopped | TpuPhase::Failed
        )
    }

    pub fn can_stop(self) -> bool {
        self.is_active()
    }

    /// Short human label for the status line.
    pub fn label(self) -> &'static str {
        match self {
            TpuPhase::Idle => "IDLE",
            TpuPhase::Verifying => "VERIFYING KAGGLE",
            TpuPhase::Queued => "WAITING FOR TPU",
            TpuPhase::Provisioning => "STARTING MODEL",
            TpuPhase::Starting => "STARTING MODEL",
            TpuPhase::LoadingWeights => "STARTING MODEL",
            TpuPhase::Compiling => "STARTING MODEL",
            TpuPhase::Healthy => "ALMOST READY",
            TpuPhase::Ready => "READY",
            TpuPhase::Stopping => "STOPPING",
            TpuPhase::Stopped => "STOPPED",
            TpuPhase::Failed => "FAILED",
        }
    }
}

/// What `kaggle kernels status` reported for the current kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KaggleStatus {
    Queued,
    Running,
    Complete,
    Error,
    Cancelled,
    NotFound,
    Unavailable,
    /// Transient CLI/network failure: unknown is NOT a terminal signal.
    Unknown,
}

impl KaggleStatus {
    pub fn parse_kaggle_output(out: &str) -> Option<KaggleStatus> {
        let idx = out
            .match_indices("KernelWorkerStatus.")
            .next()?
            .0
            + "KernelWorkerStatus.".len();
        let rest = &out[idx..];
        let word: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        match word.as_str() {
            "QUEUED" => Some(KaggleStatus::Queued),
            "RUNNING" => Some(KaggleStatus::Running),
            "COMPLETE" => Some(KaggleStatus::Complete),
            "ERROR" => Some(KaggleStatus::Error),
            "CANCELACKNOWLEDGED" => Some(KaggleStatus::Cancelled),
            "CANCELED" => Some(KaggleStatus::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoteKind {
    Info,
    Success,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityNote {
    pub ts: i64,
    pub text: String,
    pub kind: NoteKind,
}

/// One parsed launcher event (already sanitized — never contains secrets).
#[derive(Debug, Clone, PartialEq)]
pub struct LauncherEvent {
    pub ts: i64,
    pub phase: String,
    pub fields: serde_json::Value,
}

/// The reducer state. Everything the UI needs is derivable from here.
#[derive(Debug, Clone, Default)]
pub struct MachineState {
    pub phase: TpuPhase,
    pub kernel: Option<String>,
    pub topic: Option<String>,
    pub model: Option<ModelId>,
    /// Public endpoint (safe to display: it is not usable without the key).
    pub endpoint: Option<String>,
    /// False while the tunnel exists but vLLM is still booting.
    pub endpoint_live: bool,
    /// Unix seconds. `ready_at_estimated` marks derived values.
    pub ready_at: Option<i64>,
    pub ready_at_estimated: bool,
    pub queued_at: Option<i64>,
    pub allocated_at: Option<i64>,
    /// Keepalive window start (serving start). Derived from heartbeats when
    /// there is no direct timestamp — flagged estimated.
    pub serving_from: Option<i64>,
    pub serving_from_estimated: bool,
    pub keepalive_min: Option<i64>,
    pub decode_tok_s: Option<f64>,
    pub max_model_len: Option<i64>,
    pub mtp_tokens: Option<i64>,
    pub text_only: Option<bool>,
    pub error: Option<String>,
    pub activity: Vec<ActivityNote>,
    /// Set when the session reached a terminal launcher event
    /// (failed / auto-shutdown / stopped) or was reconciled to a finished
    /// kernel. Prevents any further phase movement.
    pub terminal: Option<String>,
    pub ntfy_reachable: bool,
    /// True once we fetched a full event history for this kernel.
    pub events_applied: bool,
    pub last_event_ts: i64,
    /// Consecutive endpoint probe failures (only counted while Ready).
    pub probe_fails: u32,
}

impl MachineState {
    pub fn reset_for_kernel(&mut self, kernel: &str, topic: Option<String>) {
        self.reset_for_kernel_model(kernel, topic, ModelId::Qwen38_27b);
    }

    pub fn reset_for_kernel_model(&mut self, kernel: &str, topic: Option<String>, model: ModelId) {
        *self = MachineState {
            kernel: Some(kernel.to_string()),
            topic,
            model: Some(model),
            phase: TpuPhase::Verifying,
            ntfy_reachable: true,
            ..Default::default()
        };
    }

    /// Reattach to a kernel recorded in the local state file WITHOUT
    /// fabricating a queue result: the phase stays Verifying until Kaggle or
    /// launcher events provide authoritative lifecycle evidence.
    pub fn reattach_model(&mut self, kernel: &str, topic: Option<String>, model: ModelId) {
        *self = MachineState {
            kernel: Some(kernel.to_string()),
            topic,
            model: Some(model),
            phase: TpuPhase::Verifying,
            ntfy_reachable: true,
            ..Default::default()
        };
    }

    pub(crate) fn push_note(&mut self, ts: i64, text: &str, kind: NoteKind) {
        // De-duplicate immediate repeats (e.g. duplicate "ready" re-publishes).
        if let Some(last) = self.activity.last() {
            if last.text == text && (ts - last.ts).abs() < 120 {
                return;
            }
        }
        self.activity.push(ActivityNote {
            ts,
            text: text.to_string(),
            kind,
        });
        if self.activity.len() > 60 {
            let drain = self.activity.len() - 60;
            self.activity.drain(0..drain);
        }
    }

    fn at_least(&mut self, next: TpuPhase) -> bool {
        if self.phase.rank() < next.rank() {
            self.phase = next;
            true
        } else {
            false
        }
    }

    /// Apply one launcher event. Returns false if ignored (terminal state).
    pub fn apply_event(&mut self, ev: &LauncherEvent) -> bool {
        if self.terminal.is_some() {
            return false;
        }
        let ts = ev.ts;
        let f = |k: &str| ev.fields.get(k);
        match ev.phase.as_str() {
            "install" => {
                if self.at_least(TpuPhase::Starting) {
                    self.push_note(ts, "Building the Python runtime", NoteKind::Info);
                }
            }
            "installed" => {
                if self.at_least(TpuPhase::Starting) {
                    self.push_note(ts, "Runtime ready", NoteKind::Info);
                }
            }
            "mtp-patch-applied" => self.push_note(ts, "MTP patch applied", NoteKind::Info),
            "mtp-patch-failed" => self.push_note(
                ts,
                "MTP patch failed — speculative decoding disabled",
                NoteKind::Warn,
            ),
            "cache-restored" => {
                let covers = f("covers_this_config").and_then(|v| v.as_bool()).unwrap_or(true);
                self.push_note(
                    ts,
                    if covers {
                        "XLA compile cache restored"
                    } else {
                        "XLA cache restored — cold compile for this config"
                    },
                    NoteKind::Info,
                );
            }
            "cache-missing" => self.push_note(
                ts,
                "No compile cache — cold compile (~10 min added)",
                NoteKind::Warn,
            ),
            "weights-mounted" => {
                if self.at_least(TpuPhase::LoadingWeights) {
                    self.push_note(ts, "Weights mounted", NoteKind::Info);
                }
            }
            "weights-download" => {
                if self.at_least(TpuPhase::LoadingWeights) {
                    self.push_note(ts, "Downloading weights", NoteKind::Info);
                }
            }
            "weights-downloaded" => self.push_note(ts, "Weights downloaded", NoteKind::Info),
            "loading" => {
                if self.at_least(TpuPhase::LoadingWeights) {
                    self.push_note(ts, "Loading weights", NoteKind::Info);
                }
            }
            "loaded" => {
                if self.at_least(TpuPhase::Compiling) {
                    self.push_note(ts, "Weights loaded", NoteKind::Info);
                }
            }
            "server-launch" => {
                if self.at_least(TpuPhase::Compiling) {
                    self.push_note(ts, "Starting vLLM", NoteKind::Info);
                }
                if let Some(v) = f("max_model_len").and_then(|v| v.as_i64()) {
                    self.max_model_len = Some(v);
                }
                if let Some(v) = f("mtp").and_then(|v| v.as_i64()) {
                    self.mtp_tokens = Some(v);
                }
                if let Some(v) = f("text_only").and_then(|v| v.as_bool()) {
                    self.text_only = Some(v);
                }
            }
            "tunnel-url" => {
                // Qwen reserves one URL before READY. GLM can rotate a tunnel
                // after READY when the first hostname never becomes reachable.
                if let Some(e) = f("endpoint").and_then(|v| v.as_str()) {
                    let allow_rotation = self.model == Some(ModelId::Glm53Flash);
                    if !e.is_empty()
                        && self.endpoint.as_deref() != Some(e)
                        && (!self.endpoint_live || allow_rotation)
                    {
                        self.endpoint = Some(e.to_string());
                        self.probe_fails = 0;
                        self.push_note(
                            ts,
                            if self.endpoint_live { "Endpoint rotated" } else { "Endpoint reserved" },
                            NoteKind::Info,
                        );
                    }
                }
            }
            "tunnel-failed" => self.push_note(ts, "Tunnel failed", NoteKind::Warn),
            "compiling" => {
                if self.at_least(TpuPhase::Compiling) {
                    self.push_note(ts, "Loading / compiling", NoteKind::Info);
                }
            }
            "serving" => {
                if self.at_least(TpuPhase::Healthy) {
                    self.serving_from = Some(ts);
                    self.serving_from_estimated = false;
                    self.push_note(ts, "Server healthy", NoteKind::Success);
                }
            }
            "warmed" => {
                if self.at_least(TpuPhase::Healthy) {
                    self.push_note(ts, "Engine warmed", NoteKind::Success);
                }
            }
            "ready" => {
                if let Some(e) = f("endpoint").and_then(|v| v.as_str()) {
                    if !e.is_empty() {
                        self.endpoint = Some(e.to_string());
                    }
                }
                self.endpoint_live = true;
                self.phase = TpuPhase::Ready;
                self.ready_at = Some(ts);
                self.ready_at_estimated = false;
                if self.serving_from.is_none() {
                    self.serving_from = Some(ts);
                    self.serving_from_estimated = false;
                }
                if let Some(v) = f("keepalive_min").and_then(|v| v.as_i64()) {
                    self.keepalive_min = Some(v);
                }
                if let Some(v) = f("max_model_len").and_then(|v| v.as_i64()) {
                    self.max_model_len = Some(v);
                }
                self.probe_fails = 0;
                self.push_note(ts, "Endpoint ready", NoteKind::Success);
            }
            "benchmark" => {
                if let Some(v) = f("decode_tok_s").and_then(|v| v.as_f64()) {
                    self.decode_tok_s = Some(v);
                    self.push_note(
                        ts,
                        &format!("Benchmark: {} tok/s decode", fmt_tok_s(v)),
                        NoteKind::Info,
                    );
                }
            }
            "benchmark-error" => self.push_note(ts, "Benchmark failed", NoteKind::Warn),
            "heartbeat" => {
                // up_min is the kernel's own (floored) counter since serving
                // started — the best available keepalive anchor while we do
                // not have a better one. A truncated estimate must never
                // downgrade an exact serving/ready anchor.
                if let Some(up_min) = f("up_min").and_then(|v| v.as_i64()) {
                    if self.serving_from.is_none() || self.serving_from_estimated {
                        self.serving_from = Some(ts - up_min * 60);
                        self.serving_from_estimated = true;
                    }
                } else if self.serving_from.is_none() {
                    if let Some(ra) = self.ready_at {
                        self.serving_from = Some(ra);
                        self.serving_from_estimated = true;
                    }
                }
            }
            "failed" => {
                let step = f("step").and_then(|v| v.as_str()).unwrap_or("unknown");
                self.phase = TpuPhase::Failed;
                self.terminal = Some(format!("failed-{step}"));
                self.error = Some(format!("Failed at step {step}"));
                self.push_note(ts, &format!("Failed at step {step}"), NoteKind::Error);
            }
            "auto-shutdown" => {
                self.phase = TpuPhase::Stopped;
                self.terminal = Some("auto-shutdown".into());
                self.push_note(
                    ts,
                    "Keepalive window ended — kernel shut down cleanly",
                    NoteKind::Info,
                );
            }
            "stopped" => {
                self.phase = TpuPhase::Stopped;
                self.terminal = Some("stopped".into());
                self.error = Some("Server exited unexpectedly".into());
                self.push_note(ts, "Server exited", NoteKind::Error);
            }
            "image-test" | "image-test-failed" | "probe-skip-precompile"
            | "probe-warm-restart" | "build-config-done" | "bundle-built" => {}
            other if other.starts_with('p') && other.contains('-') => {}
            _ => {}
        }
        if ev.ts > self.last_event_ts {
            self.last_event_ts = ev.ts;
        }
        true
    }

    /// Reconcile the fine-grained machine state with the coarse Kaggle kernel
    /// status. The Kaggle status can only fill gaps or close a session; it
    /// never regresses an in-progress phase.
    pub fn reconcile(&mut self, status: KaggleStatus, now: i64) {
        if self.kernel.is_none() {
            self.phase = TpuPhase::Idle;
            return;
        }
        if self.terminal.is_some() {
            return;
        }
        match status {
            KaggleStatus::Unknown => {
                // Transient CLI/network failure: keep the last known phase.
            }
            KaggleStatus::Unavailable => {
                self.push_note(
                    now,
                    "Kaggle API cannot verify this private kernel yet",
                    NoteKind::Warn,
                );
            }
            KaggleStatus::Queued => {
                if self.phase.rank() <= TpuPhase::Queued.rank() {
                    if self.phase != TpuPhase::Queued {
                        self.phase = TpuPhase::Queued;
                    }
                    if self.queued_at.is_none() {
                        self.queued_at = Some(now);
                        self.push_note(now, "Waiting for TPU", NoteKind::Info);
                    }
                }
            }
            KaggleStatus::Running => {
                if self.phase.rank() <= TpuPhase::Provisioning.rank() {
                    let first_seen = self.phase == TpuPhase::Queued;
                    self.phase = TpuPhase::Provisioning;
                    if self.allocated_at.is_none() {
                        self.allocated_at = Some(now);
                        self.push_note(
                            now,
                            if first_seen {
                                "TPU allocated"
                            } else {
                                "TPU running (events not available)"
                            },
                            NoteKind::Info,
                        );
                    }
                }
            }
            KaggleStatus::Complete | KaggleStatus::NotFound | KaggleStatus::Cancelled => {
                let terminal = match status {
                    KaggleStatus::Complete => "complete".into(),
                    KaggleStatus::NotFound => "kernel-removed".into(),
                    KaggleStatus::Cancelled => "cancelled".into(),
                    _ => unreachable!(),
                };
                let note = if self.ready_at.is_some() || self.phase.rank() >= TpuPhase::Healthy.rank() {
                    "Session ended"
                } else {
                    // Kernel is gone but we never saw it serve. Most common
                    // cause is a stale state file (auto-shutdown long ago) —
                    // treat as ended, not as a crash; an explicit ERROR status
                    // is what produces Failed.
                    "Session ended (kernel no longer running)"
                };
                self.phase = TpuPhase::Stopped;
                self.terminal = Some(terminal);
                self.push_note(now, note, NoteKind::Info);
            }
            KaggleStatus::Error => {
                self.phase = TpuPhase::Failed;
                self.terminal = Some("kaggle-error".into());
                self.error = Some("Kaggle kernel errored".into());
                self.push_note(now, "Kaggle kernel errored", NoteKind::Error);
            }
        }
    }

    /// Uptime in seconds since the server became healthy/ready.
    pub fn uptime_secs(&self, now: i64) -> Option<i64> {
        self.ready_at.map(|t| (now - t).max(0))
    }

    /// Estimated seconds remaining in the keepalive window.
    pub fn remaining_secs(&self, now: i64) -> Option<i64> {
        let keepalive = self.keepalive_min?;
        let from = self.serving_from.or(self.ready_at)?;
        let total = keepalive * 60;
        Some((total - (now - from).max(0)).max(0))
    }

    pub fn remaining_is_estimated(&self) -> bool {
        self.serving_from
            .map(|_| self.serving_from_estimated)
            .unwrap_or(true)
    }
}

fn fmt_tok_s(v: f64) -> String {
    if v.fract().abs() < 0.05 {
        format!("{}", v as i64)
    } else {
        format!("{:.1}", v)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(ts: i64, phase: &str, fields: serde_json::Value) -> LauncherEvent {
        LauncherEvent {
            ts,
            phase: phase.into(),
            fields,
        }
    }
    fn j(v: serde_json::Value) -> serde_json::Value {
        v
    }

    #[test]
    fn parse_kaggle_output_variants() {
        assert_eq!(
            KaggleStatus::parse_kaggle_output(
                "x has status \"KernelWorkerStatus.QUEUED\""
            ),
            Some(KaggleStatus::Queued)
        );
        assert_eq!(
            KaggleStatus::parse_kaggle_output("KernelWorkerStatus.RUNNING extra"),
            Some(KaggleStatus::Running)
        );
        assert_eq!(
            KaggleStatus::parse_kaggle_output("KernelWorkerStatus.CANCELACKNOWLEDGED"),
            Some(KaggleStatus::Cancelled)
        );
        assert_eq!(
            KaggleStatus::parse_kaggle_output("no status here"),
            None
        );
    }

    #[test]
    fn queued_is_not_an_error() {
        let mut m = MachineState::default();
        m.reset_for_kernel("u/k", None);
        m.reconcile(KaggleStatus::Queued, 1000);
        assert_eq!(m.phase, TpuPhase::Queued);
        assert_eq!(m.queued_at, Some(1000));
        assert!(m.error.is_none());
    }

    #[test]
    fn full_happy_path() {
        let mut m = MachineState::default();
        m.reset_for_kernel("u/k", None);
        m.reconcile(KaggleStatus::Queued, 1_000);
        m.reconcile(KaggleStatus::Running, 1_300);
        assert_eq!(m.phase, TpuPhase::Provisioning);
        assert_eq!(m.allocated_at, Some(1_300));

        m.apply_event(&ev(1_400, "install", j(serde_json::json!({}))));
        assert_eq!(m.phase, TpuPhase::Starting);
        m.apply_event(&ev(1_500, "installed", j(serde_json::json!({}))));
        m.apply_event(&ev(1_510, "mtp-patch-applied", j(serde_json::json!({}))));
        m.apply_event(&ev(1_520, "cache-restored", j(serde_json::json!({"covers_this_config": true}))));
        m.apply_event(&ev(1_530, "weights-mounted", j(serde_json::json!({}))));
        assert_eq!(m.phase, TpuPhase::LoadingWeights);
        m.apply_event(&ev(
            1_540,
            "server-launch",
            j(serde_json::json!({"max_model_len": 262144, "mtp": 3, "text_only": true})),
        ));
        assert_eq!(m.phase, TpuPhase::Compiling);
        assert_eq!(m.max_model_len, Some(262144));
        assert_eq!(m.mtp_tokens, Some(3));
        assert!(m.text_only == Some(true));

        m.apply_event(&ev(
            1_550,
            "tunnel-url",
            j(serde_json::json!({"endpoint": "https://a.b.trycloudflare.com/v1"})),
        ));
        assert_eq!(m.endpoint, Some("https://a.b.trycloudflare.com/v1".into()));
        assert!(!m.endpoint_live, "reserved endpoint must not be live yet");

        m.apply_event(&ev(2_000, "serving", j(serde_json::json!({"startup_secs": 270}))));
        assert_eq!(m.phase, TpuPhase::Healthy);

        m.apply_event(&ev(
            2_010,
            "ready",
            j(serde_json::json!({"endpoint": "https://a.b.trycloudflare.com/v1", "api_key": "sk-SECRET", "keepalive_min": 480, "max_model_len": 262144})),
        ));
        assert_eq!(m.phase, TpuPhase::Ready);
        assert!(m.endpoint_live);
        assert_eq!(m.ready_at, Some(2_010));
        assert!(!m.ready_at_estimated);
        assert_eq!(m.keepalive_min, Some(480));
    }

    #[test]
    fn ready_without_prior_tunnel() {
        let mut m = MachineState::default();
        m.reset_for_kernel("u/k", None);
        m.apply_event(&ev(
            5_000,
            "ready",
            j(serde_json::json!({"endpoint": "https://x.y/v1", "keepalive_min": 480})),
        ));
        assert_eq!(m.phase, TpuPhase::Ready);
        assert!(m.endpoint_live);
    }

    #[test]
    fn tunnel_url_after_ready_does_not_downgrade() {
        let mut m = MachineState::default();
        m.reset_for_kernel("u/k", None);
        m.apply_event(&ev(
            5_000,
            "ready",
            j(serde_json::json!({"endpoint": "https://x.y/v1", "keepalive_min": 480})),
        ));
        m.apply_event(&ev(
            5_100,
            "tunnel-url",
            j(serde_json::json!({"endpoint": "https://other.z/v1"})),
        ));
        assert_eq!(m.phase, TpuPhase::Ready);
        assert!(m.endpoint_live);
        assert_eq!(m.endpoint, Some("https://x.y/v1".into()));
    }

    #[test]
    fn failed_transition_is_terminal() {
        let mut m = MachineState::default();
        m.reset_for_kernel("u/k", None);
        m.apply_event(&ev(100, "install", j(serde_json::json!({}))));
        m.apply_event(&ev(200, "failed", j(serde_json::json!({"step": "install"}))));
        assert_eq!(m.phase, TpuPhase::Failed);
        assert!(m.terminal.is_some());
        // Later events must not move the phase.
        m.apply_event(&ev(300, "ready", j(serde_json::json!({"endpoint": "https://x.y/v1"}))));
        assert_eq!(m.phase, TpuPhase::Failed);
    }

    #[test]
    fn failed_session_is_restartable() {
        // Mirrors the frontend CAN_START set: a Failed session must offer
        // Start again (panel and tray stay consistent). Restarting a new
        // kernel clears the terminal flag.
        let mut m = MachineState::default();
        m.reset_for_kernel("u/k", None);
        m.apply_event(&ev(
            200,
            "failed",
            j(serde_json::json!({"step": "install"})),
        ));
        assert_eq!(m.phase, TpuPhase::Failed);
        assert!(m.terminal.is_some());
        assert!(
            m.phase.can_start(),
            "Failed must allow a restart, like Idle/Stopped"
        );

        // Pushing a new kernel resets the machine, terminal included.
        m.reset_for_kernel("u/k2", None);
        assert_eq!(m.phase, TpuPhase::Verifying);
        assert!(m.terminal.is_none());
    }

    #[test]
    fn can_start_phase_matrix() {
        // Start is only offered from resting states, never mid-flight.
        assert!(TpuPhase::Idle.can_start());
        assert!(TpuPhase::Stopped.can_start());
        assert!(TpuPhase::Failed.can_start());
        assert!(!TpuPhase::Queued.can_start());
        assert!(!TpuPhase::Verifying.can_start());
        assert!(!TpuPhase::Provisioning.can_start());
        assert!(!TpuPhase::Starting.can_start());
        assert!(!TpuPhase::LoadingWeights.can_start());
        assert!(!TpuPhase::Compiling.can_start());
        assert!(!TpuPhase::Healthy.can_start());
        assert!(!TpuPhase::Ready.can_start());
        assert!(!TpuPhase::Stopping.can_start());
    }

    #[test]
    fn auto_shutdown_and_stopped() {
        let mut m = MachineState::default();
        m.reset_for_kernel("u/k", None);
        m.apply_event(&ev(
            100,
            "ready",
            j(serde_json::json!({"endpoint": "https://x.y/v1", "keepalive_min": 480})),
        ));
        m.apply_event(&ev(200, "auto-shutdown", j(serde_json::json!({}))));
        assert_eq!(m.phase, TpuPhase::Stopped);
        m.apply_event(&ev(300, "ready", j(serde_json::json!({}))));
        assert_eq!(m.phase, TpuPhase::Stopped);

        let mut m2 = MachineState::default();
        m2.reset_for_kernel("u/k", None);
        m2.apply_event(&ev(100, "ready", j(serde_json::json!({"endpoint": "https://x.y/v1"}))));
        m2.apply_event(&ev(200, "stopped", j(serde_json::json!({}))));
        assert_eq!(m2.phase, TpuPhase::Stopped);
    }

    #[test]
    fn kaggle_complete_with_ready_becomes_stopped() {
        let mut m = MachineState::default();
        m.reset_for_kernel("u/k", None);
        m.apply_event(&ev(
            100,
            "ready",
            j(serde_json::json!({"endpoint": "https://x.y/v1", "keepalive_min": 480})),
        ));
        m.reconcile(KaggleStatus::Complete, 90_000);
        assert_eq!(m.phase, TpuPhase::Stopped);
    }

    #[test]
    fn kaggle_error_without_ready_becomes_failed() {
        let mut m = MachineState::default();
        m.reset_for_kernel("u/k", None);
        m.reconcile(KaggleStatus::Queued, 1_000);
        m.reconcile(KaggleStatus::Error, 2_000);
        assert_eq!(m.phase, TpuPhase::Failed);
        assert!(m.error.is_some());
    }

    #[test]
    fn unknown_kaggle_status_never_kills_session() {
        let mut m = MachineState::default();
        m.reset_for_kernel("u/k", None);
        m.apply_event(&ev(
            100,
            "ready",
            j(serde_json::json!({"endpoint": "https://x.y/v1", "keepalive_min": 480})),
        ));
        m.reconcile(KaggleStatus::Unknown, 2_000);
        assert_eq!(m.phase, TpuPhase::Ready);
    }

    #[test]
    fn unavailable_kaggle_status_stays_verifying() {
        let mut m = MachineState::default();
        m.reset_for_kernel("u/k", None);
        m.reconcile(KaggleStatus::Unavailable, 2_000);
        assert_eq!(m.phase, TpuPhase::Verifying);
        assert!(m
            .activity
            .iter()
            .any(|n| n.text.contains("cannot verify this private kernel")));
    }

    #[test]
    fn glm_tunnel_can_rotate_after_ready() {
        let mut m = MachineState::default();
        m.reset_for_kernel_model("u/glm", None, ModelId::Glm53Flash);
        m.apply_event(&ev(
            100,
            "ready",
            j(serde_json::json!({"endpoint": "https://old.trycloudflare.com", "keepalive_min": 480})),
        ));
        m.apply_event(&ev(
            120,
            "tunnel-url",
            j(serde_json::json!({"endpoint": "https://new.trycloudflare.com"})),
        ));
        assert_eq!(m.phase, TpuPhase::Ready);
        assert_eq!(m.endpoint.as_deref(), Some("https://new.trycloudflare.com"));
        assert!(m.endpoint_live);
        assert!(m.activity.iter().any(|n| n.text == "Endpoint rotated"));
    }

    #[test]
    fn glm_lifecycle_maps_into_existing_phases() {
        let mut m = MachineState::default();
        m.reset_for_kernel_model("u/glm", None, ModelId::Glm53Flash);
        m.apply_event(&ev(100, "loading", j(serde_json::json!({"note": "weights"}))));
        assert_eq!(m.phase, TpuPhase::LoadingWeights);
        m.apply_event(&ev(200, "loaded", j(serde_json::json!({"hbm_gb": 15.4}))));
        assert_eq!(m.phase, TpuPhase::Compiling);
        m.apply_event(&ev(300, "warmed", j(serde_json::json!({"minutes": 9.0}))));
        assert_eq!(m.phase, TpuPhase::Healthy);
    }

    #[test]
    fn uptime_and_remaining() {
        let mut m = MachineState::default();
        m.reset_for_kernel("u/k", None);
        m.apply_event(&ev(
            1_000,
            "ready",
            j(serde_json::json!({"endpoint": "https://x.y/v1", "keepalive_min": 480})),
        ));
        assert_eq!(m.uptime_secs(3_400), Some(2_400));
        assert_eq!(m.remaining_secs(3_400), Some(480 * 60 - 2_400));
        assert!(!m.remaining_is_estimated());

        // Heartbeat-derived serving start is flagged estimated.
        let mut m3 = MachineState::default();
        m3.reset_for_kernel("u/k", None);
        m3.apply_event(&ev(
            1_000,
            "ready",
            j(serde_json::json!({"endpoint": "https://x.y/v1"})),
        ));
        // ready already sets serving_from (not estimated). Drop it to test heartbeat path:
        m3.serving_from = None;
        m3.keepalive_min = Some(480);
        m3.apply_event(&ev(2_000, "heartbeat", j(serde_json::json!({"up_min": 10}))));
        assert_eq!(m3.serving_from, Some(1_400));
        assert!(m3.serving_from_estimated);
        assert!(m3.remaining_is_estimated());
        assert_eq!(m3.remaining_secs(3_400), Some(480 * 60 - 2_000));
    }

    #[test]
    fn heartbeat_never_downgrades_exact_anchor() {
        let mut m = MachineState::default();
        m.reset_for_kernel("u/k", None);
        m.apply_event(&ev(
            1_000,
            "ready",
            j(serde_json::json!({"endpoint": "https://x.y/v1", "keepalive_min": 480})),
        ));
        // ready established the exact serving anchor.
        assert_eq!(m.serving_from, Some(1_000));
        assert!(!m.serving_from_estimated);

        // A floored heartbeat arrives later: the exact anchor must survive
        // and the remaining counter must stay exact (no "~" in the UI).
        m.apply_event(&ev(2_000, "heartbeat", j(serde_json::json!({"up_min": 16}))));
        assert_eq!(m.serving_from, Some(1_000));
        assert!(!m.serving_from_estimated);
        assert!(!m.remaining_is_estimated());
        assert_eq!(m.remaining_secs(2_000), Some(480 * 60 - 1_000));
    }

    #[test]
    fn heartbeat_still_refines_estimated_anchor() {
        let mut m = MachineState::default();
        m.reset_for_kernel("u/k", None);
        m.apply_event(&ev(
            1_000,
            "ready",
            j(serde_json::json!({"endpoint": "https://x.y/v1", "keepalive_min": 480})),
        ));
        // Probe-recovery style anchor: present but flagged estimated.
        m.serving_from = Some(950);
        m.serving_from_estimated = true;
        m.apply_event(&ev(2_000, "heartbeat", j(serde_json::json!({"up_min": 10}))));
        assert_eq!(m.serving_from, Some(1_400));
        assert!(m.serving_from_estimated);
    }

    #[test]
    fn no_backwards_transitions() {
        let mut m = MachineState::default();
        m.reset_for_kernel("u/k", None);
        m.apply_event(&ev(100, "server-launch", j(serde_json::json!({}))));
        assert_eq!(m.phase, TpuPhase::Compiling);
        m.apply_event(&ev(200, "weights-mounted", j(serde_json::json!({}))));
        assert_eq!(m.phase, TpuPhase::Compiling, "weights after server-launch must not regress");
    }
}
