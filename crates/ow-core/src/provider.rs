//! Provider contract (docs/DAEMON_SPEC.md section 8). Network egress and secrets are added
//! with the first network provider; local providers need neither.
use serde_json::{Map, Value};
use std::time::Duration;

const MAX_PARAMS_BYTES: usize = 256;

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
    /// Request parameter names this topic accepts. Anything else is rejected; the default is
    /// none, so a topic that takes no parameters cannot be given any.
    pub params: &'static [&'static str],
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
            params: &[],
        }
    }

    pub const fn with_params(mut self, params: &'static [&'static str]) -> Self {
        self.params = params;
        self
    }

    /// Reject undeclared parameter names, non-scalar values and oversized input, so client
    /// input cannot mint unbounded cache keys.
    pub fn check_params(&self, params: &Map<String, Value>) -> Result<(), ProviderError> {
        let scalar = |v: &Value| matches!(v, Value::String(_) | Value::Number(_) | Value::Bool(_));
        let size = serde_json::to_string(params).map_or(usize::MAX, |s| s.len());
        if size > MAX_PARAMS_BYTES
            || params
                .iter()
                .any(|(k, v)| !self.params.contains(&k.as_str()) || !scalar(v))
        {
            return Err(ProviderError::BadParams);
        }
        Ok(())
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
    #[error("parameters not accepted by this topic")]
    BadParams,
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
