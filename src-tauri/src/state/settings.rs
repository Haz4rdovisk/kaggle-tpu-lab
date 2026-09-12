//! Launch profile settings (persisted in the app config dir).
//!
//! The first release ships the official defaults for this profile; the fields
//! are editable with validation so absurd combinations are rejected before
//! they ever reach `launch.py serve`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Optional explicit repo location (defaults to auto-detection).
    pub project_root: Option<String>,
    /// `--max-model-len`
    pub context: i64,
    /// `--mtp`
    pub mtp: i64,
    /// `--reasoning-effort`
    pub thinking: String,
    /// `--fast-start`
    pub fast_start: bool,
    /// `--text-only`
    pub text_only: bool,
    /// `--keepalive-min`
    pub keepalive_min: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            project_root: None,
            context: 262144,
            mtp: 3,
            thinking: "xhigh".into(),
            fast_start: true,
            text_only: true,
            keepalive_min: 480,
        }
    }
}

impl Settings {
    pub const CONTEXTS: [i64; 2] = [131072, 262144];
    pub const THINKING_LEVELS: [&str; 3] = ["xhigh", "medium", "low"];

    pub fn validate(&self) -> Result<(), String> {
        if !Self::CONTEXTS.contains(&self.context) {
            return Err(format!(
                "Context must be one of: {}",
                Self::CONTEXTS
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !(0..=5).contains(&self.mtp) {
            return Err("MTP must be between 0 and 5".into());
        }
        if !Self::THINKING_LEVELS.contains(&self.thinking.as_str()) {
            return Err(format!(
                "Thinking must be one of: {}",
                Self::THINKING_LEVELS.join(", ")
            ));
        }
        if !(30..=540).contains(&self.keepalive_min) {
            return Err("Keepalive must be between 30 and 540 min (Kaggle caps TPU sessions at 9 h)".into());
        }
        Ok(())
    }

    /// Argument vector for `launch.py serve` (tools stay enabled: no
    /// `--no-tools` flag, ever).
    pub fn serve_args(&self) -> Vec<String> {
        let mut args = vec![
            "--max-model-len".into(),
            self.context.to_string(),
            "--mtp".into(),
            self.mtp.to_string(),
            "--reasoning-effort".into(),
            self.thinking.clone(),
            "--keepalive-min".into(),
            self.keepalive_min.to_string(),
        ];
        if self.fast_start {
            args.push("--fast-start".into());
        }
        if self.text_only {
            args.push("--text-only".into());
        }
        args
    }

    fn file_path() -> Option<PathBuf> {
        let dir = dirs_config();
        dir.map(|d| d.join("settings.json"))
    }

    pub fn load() -> Settings {
        Self::file_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::file_path().ok_or("cannot resolve app config dir")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create config dir: {e}"))?;
        }
        let txt = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialize settings: {e}"))?;
        std::fs::write(&path, txt).map_err(|e| format!("cannot write settings: {e}"))
    }
}

fn dirs_config() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok()?;
    Some(PathBuf::from(home).join("AppData").join("Roaming").join("com.lucas.kaggle-tpu-companion"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_official_profile() {
        let s = Settings::default();
        assert_eq!(s.context, 262144);
        assert_eq!(s.mtp, 3);
        assert_eq!(s.thinking, "xhigh");
        assert!(s.fast_start);
        assert!(s.text_only);
        assert_eq!(s.keepalive_min, 480);
        s.validate().unwrap();
    }

    #[test]
    fn serve_args_match_profile_and_never_disable_tools() {
        let args = Settings::default().serve_args();
        let joined = args.join(" ");
        assert!(args.first().is_some());
        assert!(joined.contains("--max-model-len 262144"));
        assert!(joined.contains("--mtp 3"));
        assert!(joined.contains("--reasoning-effort xhigh"));
        assert!(joined.contains("--fast-start"));
        assert!(joined.contains("--text-only"));
        assert!(joined.contains("--keepalive-min 480"));
        assert!(!joined.contains("--no-tools"), "tools must stay enabled");
    }

    #[test]
    fn rejects_absurd_combinations() {
        let mut s = Settings::default();
        s.context = 123;
        assert!(s.validate().is_err());

        let mut s = Settings::default();
        s.mtp = 99;
        assert!(s.validate().is_err());

        let mut s = Settings::default();
        s.thinking = "crazy".into();
        assert!(s.validate().is_err());

        let mut s = Settings::default();
        s.keepalive_min = 9000;
        assert!(s.validate().is_err());
    }
}
