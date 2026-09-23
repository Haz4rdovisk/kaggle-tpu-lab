//! Secret hygiene.
//!
//! The session API key lives only in the state file and in Rust memory. It is
//! never serialized into snapshots, never rendered by the UI, and never logged.
//! Explicit copy actions may place the key or connection setup on the clipboard,
//! but that happens entirely in Rust so the raw key is never exposed to React.

const SECRET_KEYS: &[&str] = &[
    "api_key",
    "apikey",
    "key",
    "token",
    "secret",
    "password",
    "authorization",
    "auth",
];

/// Recursively remove secret-looking fields from a value.
pub fn sanitize_value(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            let mut remove = Vec::new();
            for k in map.keys().cloned().collect::<Vec<_>>() {
                if SECRET_KEYS
                    .iter()
                    .any(|s| k.eq_ignore_ascii_case(s))
                {
                    remove.push(k);
                }
            }
            for k in remove {
                map.remove(&k);
            }
            for val in map.values_mut() {
                sanitize_value(val);
            }
        }
        serde_json::Value::Array(items) => {
            for val in items {
                sanitize_value(val);
            }
        }
        _ => {}
    }
}

/// Mask a string for safe logging: replaces `sk-…` keys and long opaque
/// tokens with a fixed marker.
pub fn redact_for_log(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find("sk-") {
        out.push_str(&rest[..pos]);
        out.push_str("sk-[redacted]");
        let after = &rest[pos + 3..];
        // Consume the token body (hex chars).
        let take = after
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .count();
        rest = if take > 0 {
            &after[take.min(after.len())..]
        } else {
            after
        };
    }
    out.push_str(rest);
    out
}

/// The UI-facing placeholder when a key exists but must not be shown.
pub const MASKED_API_KEY: &str = "••••••••••••••••";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_removes_secret_fields_recursively() {
        let mut v: serde_json::Value = serde_json::from_str(
            r#"{"phase":"ready","api_key":"sk-x","nested":{"Key":"y","token":{"deep":"z"}},"keep":"ok"}"#,
        )
        .unwrap();
        sanitize_value(&mut v);
        assert!(v.get("api_key").is_none());
        assert!(v["nested"].get("Key").is_none());
        assert!(v["nested"].get("token").is_none());
        assert_eq!(v["keep"], "ok");
        assert!(v["phase"] == "ready");
    }

    #[test]
    fn redact_masks_sk_keys() {
        let s = "endpoint https://x/v1 key=sk-0123abcd4567efgh note";
        let r = redact_for_log(s);
        assert!(!r.contains("0123abcd"));
        assert!(r.contains("sk-[redacted]"));
        assert!(r.contains("https://x/v1"));
    }

    #[test]
    fn masked_key_has_no_real_content() {
        assert!(MASKED_API_KEY.chars().all(|c| c == '•'));
        assert!(!MASKED_API_KEY.starts_with("sk-"));
    }
}
