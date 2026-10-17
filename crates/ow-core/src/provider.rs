//! Provider contract (docs/DAEMON_SPEC.md section 8). Network egress and secrets are added
//! with the first network provider; local providers need neither.
use serde_json::{Map, Value};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensitivity {
    /// May be cached on disk if configured.
    Low,
    /// Memory only unless the user opts in to encrypted, time-limited persistence.
    High,
}

/// Declares a topic and the only fields it may emit; anything else is dropped.
#[derive(Debug)]
pub struct TopicSpec {
    pub name: &'static str,
    pub sensitivity: Sensitivity,
    pub interval: Duration,
    pub fields: &'static [&'static str],
}

impl TopicSpec {
    /// Keep only declared fields. This is what makes field-level grants meaningful.
    pub fn sanitize(&self, mut data: Map<String, Value>) -> Map<String, Value> {
        data.retain(|k, _| self.fields.contains(&k.as_str()));
        data
    }
}

/// Category only: never carries upstream bodies, tokens or paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("source unavailable")]
    Unavailable,
    #[error("unexpected source format")]
    Format,
    #[error("unknown topic")]
    UnknownTopic,
}

pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn topics(&self) -> &'static [TopicSpec];
    fn fetch(
        &self,
        topic: &str,
        params: &Map<String, Value>,
    ) -> Result<Map<String, Value>, ProviderError>;
}
