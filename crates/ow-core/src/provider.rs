//! Provider contract (docs/DAEMON_SPEC.md section 8). Network egress and secrets are added
//! with the first network provider; local providers need neither.
use serde_json::{Map, Value};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Sensitivity {
    /// May be cached on disk if configured.
    Low,
    /// Memory only unless the user opts in to encrypted, time-limited persistence.
    High,
}

/// Declares a topic and the only fields it may emit; anything else is dropped.
#[derive(Debug)]
#[non_exhaustive]
pub struct TopicSpec {
    pub name: &'static str,
    pub sensitivity: Sensitivity,
    pub interval: Duration,
    pub fields: &'static [&'static str],
}

impl TopicSpec {
    /// Constructor so new fields (output schema, ...) can be added without breaking providers.
    pub const fn new(
        name: &'static str,
        sensitivity: Sensitivity,
        interval: Duration,
        fields: &'static [&'static str],
    ) -> Self {
        Self {
            name,
            sensitivity,
            interval,
            fields,
        }
    }

    /// Keep only declared fields. This is what makes field-level grants meaningful.
    pub fn sanitize(&self, mut data: Map<String, Value>) -> Map<String, Value> {
        data.retain(|k, _| self.fields.contains(&k.as_str()));
        data
    }
}

/// Category only: never carries upstream bodies, tokens or paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProviderError {
    #[error("source unavailable")]
    Unavailable,
    #[error("unexpected source format")]
    Format,
    #[error("unknown topic")]
    UnknownTopic,
}

/// Synchronous on purpose: the hub runs `fetch` on its own worker threads, so a blocking
/// HTTP client with timeouts fits. Revisit (async) only if thread count becomes a problem.
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn topics(&self) -> &'static [TopicSpec];
    /// Hosts this provider may contact (enforced by the HTTP wrapper). Local providers: none.
    fn egress_hosts(&self) -> &'static [&'static str] {
        &[]
    }
    /// Vault secret names this provider may read. Local providers: none.
    fn secrets(&self) -> &'static [&'static str] {
        &[]
    }
    fn fetch(
        &self,
        topic: &str,
        params: &Map<String, Value>,
    ) -> Result<Map<String, Value>, ProviderError>;
}
