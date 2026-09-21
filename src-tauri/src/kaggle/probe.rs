//! Direct endpoint liveness probe.
//!
//! `GET {endpoint}/v1/models` with the session's Bearer key. This is the
//! authoritative "is the endpoint actually live" check when ntfy history is
//! unavailable (e.g. after >24 h or when ntfy is down). The key never leaves
//! this module and never appears in errors or logs.

pub trait EndpointProbe: Send + Sync {
    /// True when the endpoint answers the authenticated /v1/models request.
    fn probe(&self, endpoint: &str, api_key: &str) -> bool;
}

pub struct HttpProbe {
    pub agent: ureq::Agent,
}

fn models_url(endpoint: &str) -> String {
    let root = endpoint.trim_end_matches('/');
    let base = if root.ends_with("/v1") {
        root.to_string()
    } else {
        format!("{root}/v1")
    };
    format!("{base}/models")
}

impl Default for HttpProbe {
    fn default() -> Self {
        Self {
            agent: crate::kaggle::http::http_agent(),
        }
    }
}

impl EndpointProbe for HttpProbe {
    fn probe(&self, endpoint: &str, api_key: &str) -> bool {
        let url = models_url(endpoint);
        // NOTE: never log `url` failures with headers; redact everything.
        let res = self.agent
            .get(&url)
            .set("Authorization", &format!("Bearer {api_key}"))
            .call();
        match res {
            Ok(r) => r.status() == 200,
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        ok: bool,
    }

    impl EndpointProbe for Fake {
        fn probe(&self, _endpoint: &str, _api_key: &str) -> bool {
            self.ok
        }
    }

    #[test]
    fn probe_fake() {
        assert!(Fake { ok: true }.probe("https://x/v1", "sk-abc"));
        assert!(!Fake { ok: false }.probe("https://x/v1", "sk-abc"));
    }

    #[test]
    fn normalizes_qwen_and_glm_endpoint_shapes() {
        assert_eq!(models_url("https://qwen.example/v1"), "https://qwen.example/v1/models");
        assert_eq!(models_url("https://glm.example"), "https://glm.example/v1/models");
        assert_eq!(models_url("https://glm.example/"), "https://glm.example/v1/models");
    }
}
