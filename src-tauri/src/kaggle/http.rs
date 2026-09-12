//! Shared ureq `Agent` with per-operation timeouts.
//!
//! IMPORTANT — use `timeout_connect` / `timeout_read` / `timeout_write`
//! (idle timeouts per socket operation) rather than the global `.timeout()`
//! deadline. On networks with a latency-adding middlebox, the global
//! deadline aborts otherwise-healthy slow responses ("timed out reading
//! response"), whereas a per-read idle timeout tolerates slow-but-flowing
//! delivery. Verified empirically: the global deadline could not read even
//! `http://example.com`, while the per-read agent reads ntfy + the endpoint
//! probe reliably.

use std::time::Duration;

/// A single shared client. Connection pooling is internal to the agent.
pub fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        // Idle time between chunks of the response body. ntfy history and the
        // vLLM /models call can be slow on a loaded tunnel; be generous.
        .timeout_read(Duration::from_secs(25))
        .timeout_write(Duration::from_secs(10))
        .build()
}
