//! Thin wrapper around the existing `launch.py` CLI.
//!
//! We reuse the launcher exactly as documented:
//!   - `launch.py serve <profile args>`  pushes the kernel and writes the
//!     state file, then watches (we detach after the push, like Ctrl-C).
//!   - `launch.py status`                one-shot kernel status + events.
//!   - `launch.py stop`                  official stop (kernel delete,
//!     non-interactive: it pipes its own "yes").
//!
//! The launcher is only ever invoked with argument vectors assembled here.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::process;
use crate::state::machine::KaggleStatus;
use crate::state::model::ModelId;

const STATUS_TIMEOUT: Duration = Duration::from_secs(45);
const STOP_TIMEOUT: Duration = Duration::from_secs(240);

#[derive(Debug, Clone)]
pub struct Launcher {
    pub python: PathBuf,
    pub project_root: PathBuf,
}

/// The local state file written by launch.py after a successful push:
/// `{"kernel": "user/slug", "topic": "ktl-...", "api_key": "sk-...", "model": "qwen38-27b"}`.
#[derive(Debug, Clone, Default)]
pub struct LocalState {
    pub kernel: String,
    pub topic: String,
    pub model: ModelId,
    pub submitted_at: Option<i64>,
    pub keepalive_min: Option<i64>,
    /// Presence only is exposed to the UI; the value stays in Rust memory.
    pub has_api_key: bool,
}

impl LocalState {
    /// Read and parse the state file. The api key is read into memory by the
    /// caller via [`raw_api_key`] — this struct never holds it.
    pub fn read(path: &Path) -> Option<LocalState> {
        let text = std::fs::read_to_string(path).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        let model = state_model(&v)?;
        let kernel = v.get("kernel")?.as_str()?.to_string();
        if kernel.is_empty() {
            return None;
        }
        let topic = v
            .get("topic")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_string();
        let has_api_key = v
            .get("api_key")
            .and_then(|k| k.as_str())
            .map(|k| !k.is_empty())
            .unwrap_or(false);
        let submitted_at = v.get("submitted_at").and_then(|v| v.as_i64());
        let keepalive_min = v.get("keepalive_min").and_then(|v| v.as_i64());
        Some(LocalState {
            kernel,
            topic,
            model,
            submitted_at,
            keepalive_min,
            has_api_key,
        })
    }

    /// Extract the raw api key for in-memory use only. Never log it.
    pub fn raw_api_key(path: &Path) -> Option<String> {
        let text = std::fs::read_to_string(path).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        state_model(&v)?;
        v.get("api_key")
            .and_then(|k| k.as_str())
            .filter(|k| !k.is_empty())
            .map(|k| k.to_string())
    }

    pub fn age_secs(&self, path: &Path, now: i64) -> Option<i64> {
        let submitted_at = self.submitted_at.or_else(|| {
            std::fs::metadata(path)
                .ok()?
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs() as i64)
        })?;
        Some((now - submitted_at).max(0))
    }
}

fn state_model(v: &serde_json::Value) -> Option<ModelId> {
    v.get("model")
        .and_then(|m| m.as_str())
        .map(ModelId::parse_cli_id)
        .unwrap_or(Some(ModelId::Qwen38_27b))
}

impl Launcher {
    /// Resolve the launcher location:
    ///  1. explicit override (settings / tests),
    ///  2. walk up from the executable until `launch.py` is found,
    ///  3. the conventional `~/.Dev-projects/kaggle-tpu-lab`.
    pub fn resolve(explicit_root: Option<&str>) -> std::io::Result<Launcher> {
        let explicit = explicit_root.map(PathBuf::from).filter(|p| p.is_dir());
        let from_exe = Self::exe_dir().and_then(|d| Self::find_repo_from(&d));
        let root = match (explicit, from_exe) {
            (Some(r), _) => r,
            (_, Some(r)) => r,
            (None, None) => Self::conventional_root()?,
        };
        Self::from_root(&root)
    }

    fn exe_dir() -> Option<PathBuf> {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
    }

    fn from_root(root: &Path) -> std::io::Result<Launcher> {
        if !root.join("launch.py").exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("launch.py not found under {}", root.display()),
            ));
        }
        let python = if cfg!(windows) {
            root.join(".venv").join("Scripts").join("python.exe")
        } else {
            root.join(".venv").join("bin").join("python")
        };
        if !python.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("venv python not found at {}", python.display()),
            ));
        }
        Ok(Launcher {
            python,
            project_root: root.to_path_buf(),
        })
    }

    fn find_repo_from(start: &Path) -> Option<PathBuf> {
        let mut cur = Some(start);
        while let Some(dir) = cur {
            if dir.join("launch.py").exists() {
                return Some(dir.to_path_buf());
            }
            cur = dir.parent();
        }
        None
    }

    fn conventional_root() -> std::io::Result<PathBuf> {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "cannot determine home directory",
                )
            })?;
        Ok(Path::new(&home).join(".Dev-projects").join("kaggle-tpu-lab"))
    }

    fn launch_py(&self) -> PathBuf {
        self.project_root.join("launch.py")
    }

    /// `kaggle kernels status <kernel>` through the project venv, exactly like
    /// launch.py does (`python -m kaggle ...`).
    pub fn kernel_status(&self, kernel: &str) -> KaggleStatus {
        let out = process::run_capture(
            &self.python,
            &["-m".into(), "kaggle".into(), "kernels".into(), "status".into(), kernel.into()],
            &self.project_root,
            None,
            STATUS_TIMEOUT,
        );
        match out {
            Ok((_code, output)) => {
                if let Some(st) = KaggleStatus::parse_kaggle_output(&output) {
                    st
                } else if looks_unavailable(&output) {
                    KaggleStatus::Unavailable
                } else if looks_not_found(&output) {
                    KaggleStatus::NotFound
                } else {
                    // CLI or network hiccup: not a terminal signal.
                    KaggleStatus::Unknown
                }
            }
            Err(_) => KaggleStatus::Unknown,
        }
    }

    /// Start the launcher's serve flow detached. The process keeps running its
    /// watch loop until we kill it once the state file shows the new push.
    pub fn serve(&self, args: &[String]) -> std::io::Result<std::process::Child> {
        process::spawn_detached(
            &self.python,
            &[self.launch_py().display().to_string(), "serve".into()]
                .into_iter()
                .chain(args.iter().cloned())
                .collect::<Vec<_>>(),
            &self.project_root,
        )
    }

    /// Official stop: `launch.py stop` (kernel delete, self-confirming).
    pub fn stop(&self) -> Result<String, String> {
        let (code, output) = process::run_capture(
            &self.python,
            &[self.launch_py().display().to_string(), "stop".into()],
            &self.project_root,
            None,
            STOP_TIMEOUT,
        )
        .map_err(|e| format!("stop timed out or failed to start: {e}"))?;
        let out = crate::security::redact_for_log(&output);
        if code == 0 {
            Ok(out)
        } else {
            Err(format!("launch.py stop exited {code}: {}", tail(&out)))
        }
    }
}

fn looks_unavailable(output: &str) -> bool {
    let lower = output.to_lowercase();
    lower.contains("cannot access kernel") || lower.contains("kernels.get")
}

fn looks_not_found(output: &str) -> bool {
    let lower = output.to_lowercase();
    lower.contains("not found")
        || lower.contains("no such kernel")
        || lower.contains("404")
        || lower.contains("does not exist")
}

fn tail(s: &str) -> String {
    let s = s.trim();
    if s.len() <= 400 {
        s.to_string()
    } else {
        format!("…{}", &s[s.len() - 400..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_state_parses_and_hides_key() {
        let dir = tempfile_dir();
        let path = dir.join("state.json");
        std::fs::write(
            &path,
            r#"{"kernel":"u/k","topic":"ktl-abc","api_key":"sk-SECRET"}"#,
        )
        .unwrap();
        let st = LocalState::read(&path).expect("state");
        assert_eq!(st.kernel, "u/k");
        assert_eq!(st.topic, "ktl-abc");
        assert_eq!(st.model, ModelId::Qwen38_27b);
        assert_eq!(st.submitted_at, None);
        assert_eq!(st.keepalive_min, None);
        assert!(st.has_api_key);
        assert_eq!(LocalState::raw_api_key(&path).as_deref(), Some("sk-SECRET"));

        // The serialized LocalState must not contain the key.
        let serialized = format!("{st:?}");
        assert!(!serialized.contains("SECRET"));
    }

    #[test]
    fn local_state_accepts_glm_sessions() {
        let dir = tempfile_dir();
        let path = dir.join("glm-state.json");
        std::fs::write(
            &path,
            r#"{"kernel":"u/glm","topic":"ktl-abc","api_key":"glm-SECRET","model":"glm53-flash"}"#,
        )
        .unwrap();
        let st = LocalState::read(&path).expect("glm state");
        assert_eq!(st.model, ModelId::Glm53Flash);
        assert_eq!(LocalState::raw_api_key(&path).as_deref(), Some("glm-SECRET"));
    }

    #[test]
    fn local_state_rejects_unknown_models() {
        let dir = tempfile_dir();
        let path = dir.join("unknown-state.json");
        std::fs::write(
            &path,
            r#"{"kernel":"u/x","topic":"ktl-abc","api_key":"secret","model":"future-model"}"#,
        )
        .unwrap();
        assert!(LocalState::read(&path).is_none());
        assert!(LocalState::raw_api_key(&path).is_none());
    }

    #[test]
    fn local_state_missing_or_invalid() {
        let dir = tempfile_dir();
        let missing = dir.join("nope.json");
        assert!(LocalState::read(&missing).is_none());
        let bad = dir.join("bad.json");
        std::fs::write(&bad, "not json").unwrap();
        assert!(LocalState::read(&bad).is_none());
    }

    #[test]
    fn private_kernel_access_error_is_unavailable_not_missing() {
        let out = "Cannot access kernel 'u/k' (kernels.get failed).";
        assert!(looks_unavailable(out));
        assert!(!looks_not_found(out));
    }

    #[test]
    fn local_state_uses_explicit_submission_time_for_age() {
        let dir = tempfile_dir();
        let path = dir.join("aged-state.json");
        std::fs::write(
            &path,
            r#"{"kernel":"u/k","topic":"ktl-x","api_key":"x","submitted_at":1000,"keepalive_min":480}"#,
        )
        .unwrap();
        let st = LocalState::read(&path).expect("state");
        assert_eq!(st.age_secs(&path, 1_600), Some(600));
        assert_eq!(st.keepalive_min, Some(480));
    }

    fn tempfile_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ktl-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d
    }
}
