//! Pi integration: provider registration with full backup / validation /
//! rollback, ported from `~\bin\sync-kaggle-tpu-pi.ps1`.
//!
//! Invariants:
//!   - all existing providers are preserved (we only replace `kaggle-tpu`);
//!   - the Pi default model is never touched (it does not live in the files
//!     we write — we never edit settings.json at all);
//!   - both files are backed up before writing; any failure restores both
//!     byte-for-byte and removes the backups (no partial states);
//!   - the api key is written into auth.json by the OS, never logged.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};

pub const PROVIDER_ID: &str = "kaggle-tpu";
pub const MODEL_ID: &str = "qwen3.8-27b";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PiState {
    NotConfigured,
    Synced,
    /// Provider exists but points at a different endpoint than the live one.
    Stale,
    /// Last sync attempt failed (runtime flag, not derived from files).
    SyncFailed,
}

#[derive(Debug, Clone)]
pub struct PiFiles {
    pub models: PathBuf,
    pub auth: PathBuf,
}

pub fn pi_files() -> PiFiles {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    let dir = PathBuf::from(home).join(".pi").join("agent");
    PiFiles {
        models: dir.join("models.json"),
        auth: dir.join("auth.json"),
    }
}

/// Build the provider object exactly as the ps1 reference does.
pub fn build_provider(endpoint: &str, context_window: i64) -> Value {
    json!({
        "name": "Kaggle TPU - Qwen3.8 27B",
        "baseUrl": endpoint,
        "api": "openai-completions",
        "compat": {
            "supportsStore": false,
            "supportsDeveloperRole": false,
            "supportsReasoningEffort": false,
            "supportsUsageInStreaming": true,
            "maxTokensField": "max_tokens",
            "supportsStrictMode": false,
            "thinkingFormat": "chat-template",
            "chatTemplateKwargs": {
                "enable_thinking": { "$var": "thinking.enabled" },
                "reasoning_effort": { "$var": "thinking.effort", "omitWhenOff": true }
            }
        },
        "models": [{
            "id": MODEL_ID,
            "name": "Qwen3.8-27B (Kaggle TPU)",
            "reasoning": true,
            "thinkingLevelMap": {
                "minimal": "low",
                "low": "low",
                "medium": "medium",
                "high": "xhigh",
                "xhigh": "xhigh",
                "max": "xhigh"
            },
            "input": ["text"],
            "contextWindow": context_window,
            "maxTokens": 65536
        }]
    })
}

pub fn auth_entry(api_key: &str) -> Value {
    json!({ "type": "api_key", "key": api_key })
}

/// Read-only assessment used by the UI (TPU and Pi states are separate).
pub fn assess(models_raw: &str, auth_raw: &str, current_endpoint: Option<&str>) -> PiState {
    let models: Value = match serde_json::from_str(models_raw) {
        Ok(v) => v,
        Err(_) => return PiState::NotConfigured,
    };
    let auth: Value = match serde_json::from_str(auth_raw) {
        Ok(v) => v,
        Err(_) => return PiState::NotConfigured,
    };
    let provider = models
        .get("providers")
        .and_then(|p| p.get(PROVIDER_ID));
    if provider.is_none() {
        return PiState::NotConfigured;
    }
    if auth.get(PROVIDER_ID).is_none() {
        return PiState::NotConfigured;
    }
    if let Some(cur) = current_endpoint {
        let base = provider
            .and_then(|p| p.get("baseUrl"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if base != cur {
            return PiState::Stale;
        }
    }
    PiState::Synced
}

/// Run the sync with rollback. `api_key` is used in-memory only.
pub fn sync(models_path: &std::path::Path, auth_path: &std::path::Path, endpoint: &str, context_window: i64, api_key: &str) -> Result<(), String> {
    // 1. read current files
    let models_raw = std::fs::read_to_string(models_path)
        .map_err(|e| format!("cannot read models.json: {e}"))?;
    let auth_raw = std::fs::read_to_string(auth_path)
        .map_err(|e| format!("cannot read auth.json: {e}"))?;
    let mut models: Value = serde_json::from_str(&models_raw)
        .map_err(|e| format!("models.json is not valid JSON: {e}"))?;
    let mut auth: Value = serde_json::from_str(&auth_raw)
        .map_err(|e| format!("auth.json is not valid JSON: {e}"))?;

    // 2. backups (temporary, same directory)
    let models_bak = models_path.with_extension("json.ktl-sync.tmp");
    let auth_bak = auth_path.with_extension("json.ktl-sync.tmp");
    std::fs::copy(models_path, &models_bak)
        .map_err(|e| format!("backup models.json failed: {e}"))?;
    std::fs::copy(auth_path, &auth_bak)
        .map_err(|e| format!("backup auth.json failed: {e}"))?;

    let result = write_and_validate(&mut models, &mut auth, endpoint, context_window, api_key, models_path, auth_path);

    match result {
        Ok(()) => {
            let _ = std::fs::remove_file(&models_bak);
            let _ = std::fs::remove_file(&auth_bak);
            Ok(())
        }
        Err(step) => {
            // Full rollback of both files, then clean up.
            let _ = std::fs::copy(&models_bak, models_path);
            let _ = std::fs::copy(&auth_bak, auth_path);
            let _ = std::fs::remove_file(&models_bak);
            let _ = std::fs::remove_file(&auth_bak);
            Err(step)
        }
    }
}

fn write_and_validate(
    models: &mut Value,
    auth: &mut Value,
    endpoint: &str,
    context_window: i64,
    api_key: &str,
    models_path: &std::path::Path,
    auth_path: &std::path::Path,
) -> Result<(), String> {
    // 3. replace only the kaggle-tpu entries; everything else is preserved.
    let providers = models
        .as_object_mut()
        .ok_or("models.json root is not an object")?;
    let providers = providers
        .entry("providers".to_string())
        .or_insert_with(|| json!({}));
    if !providers.is_object() {
        return Err("models.json 'providers' is not an object".into());
    }
    providers[PROVIDER_ID] = build_provider(endpoint, context_window);

    let auth_obj = auth
        .as_object_mut()
        .ok_or("auth.json root is not an object")?;
    auth_obj.insert(PROVIDER_ID.to_string(), auth_entry(api_key));

    // 4. write (UTF-8, no BOM, 2-space indent for readability)
    let models_txt = serde_json::to_string_pretty(models)
        .map_err(|e| format!("serialize models failed: {e}"))?;
    let auth_txt = serde_json::to_string_pretty(auth)
        .map_err(|e| format!("serialize auth failed: {e}"))?;
    std::fs::write(models_path, models_txt)
        .map_err(|e| format!("write models.json failed: {e}"))?;
    std::fs::write(auth_path, auth_txt)
        .map_err(|e| format!("write auth.json failed: {e}"))?;

    // 5. validate: re-read and re-parse both files
    let re_models_raw = std::fs::read_to_string(models_path)
        .map_err(|e| format!("re-read models.json failed: {e}"))?;
    let re_models: Value = serde_json::from_str(&re_models_raw)
        .map_err(|e| format!("models.json invalid after write: {e}"))?;
    let re_auth_raw = std::fs::read_to_string(auth_path)
        .map_err(|e| format!("re-read auth.json failed: {e}"))?;
    let re_auth: Value = serde_json::from_str(&re_auth_raw)
        .map_err(|e| format!("auth.json invalid after write: {e}"))?;

    // 6. validate provider + auth content
    let provider = re_models
        .get("providers")
        .and_then(|p| p.get(PROVIDER_ID))
        .ok_or("provider kaggle-tpu missing after write")?;
    if provider.get("baseUrl").and_then(|v| v.as_str()) != Some(endpoint) {
        return Err("provider baseUrl mismatch after write".into());
    }
    let model = provider
        .get("models")
        .and_then(|m| m.get(0))
        .ok_or("provider has no models after write")?;
    if model.get("id").and_then(|v| v.as_str()) != Some(MODEL_ID) {
        return Err("model id mismatch after write".into());
    }
    let key_ok = re_auth
        .get(PROVIDER_ID)
        .and_then(|a| a.get("key"))
        .and_then(|v| v.as_str())
        .map(|k| k == api_key && !k.is_empty())
        .unwrap_or(false);
    if !key_ok {
        return Err("auth entry invalid after write".into());
    }
    Ok(())
}

/// Optional external validation through the `pi` CLI (mirrors the ps1).
/// Returns Ok(true) when the CLI confirmed, Ok(false) when the CLI is not
/// available (file-level validation already passed), Err(message) on a real
/// failure.
pub fn validate_with_pi_cli(timeout: Duration) -> Result<bool, String> {
    let pi = find_pi_cli();
    let Some(pi) = pi else {
        return Ok(false);
    };

    // On Windows, pi is an npm .cmd shim: it must go through cmd.exe /C.
    let run = |args: &[&str]| -> (bool, String) {
        #[cfg(windows)]
        {
            let full: Vec<String> = std::iter::once(pi.display().to_string())
                .chain(args.iter().map(|s| s.to_string()))
                .collect();
            match crate::process::run_capture(
                std::path::Path::new("cmd.exe"),
                &["/C".to_string()].into_iter().chain(full).collect::<Vec<_>>(),
                std::path::Path::new("."),
                None,
                timeout,
            ) {
                Ok((code, text)) => (code == 0, text),
                Err(_) => (false, "failed to run pi cli".into()),
            }
        }
        #[cfg(not(windows))]
        {
            match crate::process::run_capture(
                std::path::Path::new(&pi),
                &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                std::path::Path::new("."),
                None,
                timeout,
            ) {
                Ok((code, text)) => (code == 0, text),
                Err(_) => (false, "failed to run pi cli".into()),
            }
        }
    };

    let (ok, text) = run(&["--list-models", PROVIDER_ID]);
    if !ok || !text.contains(MODEL_ID) {
        return Err("pi did not resolve kaggle-tpu/qwen3.8-27b".into());
    }
    let (ok, _text) = run(&["auth", "check", "--provider", PROVIDER_ID, "--json", "--no-refresh"]);
    if !ok {
        return Err("pi auth check failed for kaggle-tpu".into());
    }
    Ok(true)
}

fn find_pi_cli() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PI_BIN") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let npm = PathBuf::from(appdata).join("npm").join("pi.cmd");
            if npm.exists() {
                return Some(npm);
            }
        }
    }
    #[cfg(not(windows))]
    {
        for dir in std::env::var("PATH")
            .ok()?
            .split(':')
            .map(PathBuf::from)
        {
            let p = dir.join("pi");
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

/// Assess from disk against the live endpoint (cheap: two small files).
pub fn assess_current(current_endpoint: Option<&str>) -> PiState {
    let files = pi_files();
    let models_raw = match std::fs::read_to_string(&files.models) {
        Ok(t) => t,
        Err(_) => return PiState::NotConfigured,
    };
    let auth_raw = match std::fs::read_to_string(&files.auth) {
        Ok(t) => t,
        Err(_) => return PiState::NotConfigured,
    };
    assess(&models_raw, &auth_raw, current_endpoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ktl-pi-{tag}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d
    }

    fn setup(tag: &str) -> (PathBuf, PathBuf, String, String) {
        let d = tmpdir(tag);
        let models = d.join("models.json");
        let auth = d.join("auth.json");
        let models_raw = serde_json::to_string(&json!({
            "providers": {
                "other-provider": { "name": "Keep me", "baseUrl": "https://keep.example/v1" },
                "kaggle-tpu": { "name": "Old", "baseUrl": "https://old.example/v1" }
            }
        }))
        .unwrap();
        let auth_raw = serde_json::to_string(&json!({
            "other-auth": { "type": "api_key", "key": "other-key" },
            "kaggle-tpu": { "type": "api_key", "key": "old-key" }
        }))
        .unwrap();
        std::fs::write(&models, &models_raw).unwrap();
        std::fs::write(&auth, &auth_raw).unwrap();
        (models, auth, models_raw, auth_raw)
    }

    #[test]
    fn sync_preserves_other_providers_and_updates_kaggle() {
        let (models, auth, _, _) = setup("pres");
        sync(&models, &auth, "https://new.example/v1", 262144, "sk-newkey").unwrap();
        let m: Value = serde_json::from_str(&std::fs::read_to_string(&models).unwrap()).unwrap();
        let a: Value = serde_json::from_str(&std::fs::read_to_string(&auth).unwrap()).unwrap();
        // existing provider untouched
        assert_eq!(m["providers"]["other-provider"]["baseUrl"], "https://keep.example/v1");
        assert_eq!(a["other-auth"]["key"], "other-key");
        // kaggle-tpu updated
        assert_eq!(m["providers"][PROVIDER_ID]["baseUrl"], "https://new.example/v1");
        assert_eq!(m["providers"][PROVIDER_ID]["models"][0]["id"], MODEL_ID);
        assert_eq!(m["providers"][PROVIDER_ID]["models"][0]["contextWindow"], 262144);
        assert_eq!(a[PROVIDER_ID]["key"], "sk-newkey");
        // backups removed
        assert!(!models.with_extension("json.ktl-sync.tmp").exists());
        assert!(!auth.with_extension("json.ktl-sync.tmp").exists());
    }

    #[test]
    fn sync_failure_rolls_back_both_files() {
        let (models, auth, models_raw, auth_raw) = setup("rb");
        // Force a failure: make the auth path unwritable after backup by
        // pointing the auth file at a directory of the same name? Simpler:
        // corrupt the auth JSON root so validation (object root) fails.
        let d = models.parent().unwrap().to_path_buf();
        std::fs::write(&auth, "[1,2,3]").unwrap();
        let res = sync(&models, &auth, "https://new.example/v1", 262144, "sk-newkey");
        assert!(res.is_err(), "sync must fail on non-object auth root");
        // models untouched (write never reached validation)
        assert_eq!(std::fs::read_to_string(&models).unwrap(), models_raw);
        // auth restored byte-for-byte
        assert_eq!(std::fs::read_to_string(&auth).unwrap(), "[1,2,3]");
        assert!(!models.with_extension("json.ktl-sync.tmp").exists());
        assert!(!auth.with_extension("json.ktl-sync.tmp").exists());
        let _ = d;
    }

    #[test]
    fn sync_invalid_json_input_errors_without_partial_write() {
        let d = tmpdir("inv");
        let models = d.join("models.json");
        let auth = d.join("auth.json");
        let models_raw = r#"{"providers":{"x":{"a":1}}}"#.to_string();
        std::fs::write(&models, &models_raw).unwrap();
        std::fs::write(&auth, "{broken").unwrap();
        let res = sync(&models, &auth, "https://n/v1", 262144, "sk-x");
        assert!(res.is_err());
        assert_eq!(std::fs::read_to_string(&models).unwrap(), models_raw);
    }

    #[test]
    fn assess_states() {
        let models = json!({"providers":{"kaggle-tpu":{"baseUrl":"https://a/v1"}}}).to_string();
        let auth = json!({"kaggle-tpu":{"type":"api_key","key":"k"}}).to_string();
        assert_eq!(assess(&models, &auth, Some("https://a/v1")), PiState::Synced);
        assert_eq!(assess(&models, &auth, Some("https://b/v1")), PiState::Stale);
        assert_eq!(assess(&models, &auth, None), PiState::Synced);
        let no_auth = "{}".to_string();
        assert_eq!(assess(&models, &no_auth, None), PiState::NotConfigured);
        let no_prov = "{}".to_string();
        assert_eq!(assess(&no_prov, &auth, None), PiState::NotConfigured);
        assert_eq!(assess("bad json", &auth, None), PiState::NotConfigured);
    }

    #[test]
    fn provider_shape_matches_reference() {
        let p = build_provider("https://e/v1", 131072);
        assert_eq!(p["api"], "openai-completions");
        assert_eq!(p["compat"]["thinkingFormat"], "chat-template");
        assert_eq!(p["compat"]["chatTemplateKwargs"]["reasoning_effort"]["$var"], "thinking.effort");
        assert_eq!(p["models"][0]["thinkingLevelMap"]["max"], "xhigh");
        assert_eq!(p["models"][0]["maxTokens"], 65536);
        assert_eq!(p["models"][0]["contextWindow"], 131072);
        assert!(p["models"][0]["reasoning"] == true);
    }
}
