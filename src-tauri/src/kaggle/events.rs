//! ntfy event parsing.
//!
//! ntfy delivers NDJSON lines: `{"event":"message","time":<unix>,"message":"<json>"}`
//! where the inner message is the launcher's `publish(phase, **extra)` payload.
//! Parsing is deliberately tolerant: a malformed line must never take the
//! poller down, and ntfy is auxiliary (see state/machine.rs).

use crate::security;
use crate::state::machine::LauncherEvent;

/// Parse a raw ntfy `/json?poll=1` response body into launcher events.
pub fn parse_ntfy_body(body: &str) -> Vec<LauncherEvent> {
    let mut out = Vec::new();
    for line in body.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let outer: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if outer.get("event").and_then(|v| v.as_str()) != Some("message") {
            continue;
        }
        let ts = outer
            .get("time")
            .and_then(|v| v.as_i64())
            .unwrap_or_default();
        let inner_raw = outer.get("message").and_then(|v| v.as_str()).unwrap_or("{}");
        let mut inner: serde_json::Value = match serde_json::from_str(inner_raw) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let phase = inner
            .get("phase")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if phase.is_empty() {
            continue;
        }
        // Strip anything secret-looking before the event leaves this module.
        security::sanitize_value(&mut inner);
        out.push(LauncherEvent {
            ts,
            phase,
            fields: inner,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_messages_and_skips_junk() {
        let body = "\
{\"event\":\"message\",\"time\":100,\"message\":\"{\\\"phase\\\":\\\"ready\\\",\\\"endpoint\\\":\\\"https://x.y/v1\\\",\\\"api_key\\\":\\\"sk-SECRET\\\",\\\"keepalive_min\\\":480}\"}
not json at all
{\"event\":\"error\",\"time\":101,\"message\":\"nope\"}
{\"event\":\"message\",\"time\":102,\"message\":\"{\\\"phase\\\":\\\"heartbeat\\\",\\\"up_min\\\":10}\"}
{\"event\":\"message\",\"time\":103,\"message\":\"{broken\"}
";
        let events = parse_ntfy_body(body);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].ts, 100);
        assert_eq!(events[0].phase, "ready");
        assert_eq!(events[1].phase, "heartbeat");
        assert_eq!(events[1].fields["up_min"], 10);
    }

    #[test]
    fn api_key_never_survives_parsing() {
        let body = "{\"event\":\"message\",\"time\":1,\"message\":\"{\\\"phase\\\":\\\"ready\\\",\\\"api_key\\\":\\\"sk-abc\\\",\\\"nested\\\":{\\\"key\\\":\\\"nested-secret\\\",\\\"deep\\\":{\\\"token\\\":\\\"t\\\"}}}\"}";
        let events = parse_ntfy_body(body);
        assert_eq!(events.len(), 1);
        let s = events[0].fields.to_string();
        assert!(!s.contains("sk-abc"), "secret leaked into parsed event: {s}");
        assert!(!s.contains("nested-secret"));
        assert!(!s.contains("\"t\""));
    }
}
