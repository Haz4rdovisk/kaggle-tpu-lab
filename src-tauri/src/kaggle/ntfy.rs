//! ntfy transport (auxiliary source of truth).
//!
//! The kernel publishes structured events to `ntfy.sh/<topic>`. We poll the
//! JSON feed. Failures are returned as Err and the state machine ignores
//! them: an ntfy outage must never mark a RUNNING kernel as dead.

use crate::kaggle::events;
use crate::state::machine::LauncherEvent;

/// Abstraction so tests can feed synthetic event histories.
pub trait EventSource: Send + Sync {
    fn fetch(&self, topic: &str, since: i64) -> Result<Vec<LauncherEvent>, String>;
}

pub struct NtfySource {
    pub agent: ureq::Agent,
}

impl Default for NtfySource {
    fn default() -> Self {
        Self {
            agent: crate::kaggle::http::http_agent(),
        }
    }
}

impl EventSource for NtfySource {
    fn fetch(&self, topic: &str, since: i64) -> Result<Vec<LauncherEvent>, String> {
        let url = format!("https://ntfy.sh/{topic}/json?poll=1&since={since}");
        let body = self.agent.get(&url)
            .call()
            .map_err(|e| crate::security::redact_for_log(&e.to_string()))?
            .into_string()
            .map_err(|e| e.to_string())?;
        Ok(events::parse_ntfy_body(&body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        events: Vec<LauncherEvent>,
        fail: bool,
    }

    impl EventSource for Fake {
        fn fetch(&self, _topic: &str, _since: i64) -> Result<Vec<LauncherEvent>, String> {
            if self.fail {
                Err("ntfy unreachable".into())
            } else {
                Ok(self.events.clone())
            }
        }
    }

    #[test]
    fn source_trait_works() {
        let src = Fake {
            events: vec![LauncherEvent {
                ts: 5,
                phase: "ready".into(),
                fields: serde_json::json!({}),
            }],
            fail: false,
        };
        assert_eq!(src.fetch("t", 0).unwrap().len(), 1);
        let bad: Fake = Default::default();
        let bad = Fake {
            events: vec![],
            fail: true,
        };
        assert!(bad.fetch("t", 0).is_err());
    }
}
