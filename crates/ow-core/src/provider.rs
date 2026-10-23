//! Provider contract (docs/DAEMON_SPEC.md section 8). Secrets reach a provider only through
//! `FetchCtx` as `SecretHandle`s; network egress is added with the first network provider.
//! Local providers need neither.
use serde_json::{Map, Value};
use std::fmt;
use std::time::Duration;
use zeroize::Zeroizing;

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
    /// The vault is locked. Not a failure of the source: the hub does not back off for it.
    #[error("credentials locked")]
    Locked,
    /// The provider asked for a secret it did not declare in `secrets()`, or one that is not
    /// in the vault.
    #[error("credential not available")]
    NoSecret,
}

/// A secret lent to one `fetch` call. Zeroized on drop. Deliberately no `Display`, `Clone`,
/// `Serialize` or `PartialEq`, and `Debug` is redacted, so the value cannot reach a log,
/// an error or a reply by formatting. `expose` is the only way to read it, and the future
/// egress wrapper (docs/AUTH_PLAN.md section 3.2) is meant to be its only caller.
pub struct SecretHandle(Zeroizing<String>);

impl SecretHandle {
    pub fn new(value: &str) -> Self {
        SecretHandle(Zeroizing::new(value.to_owned()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretHandle(<redacted>)")
    }
}

/// Where handles come from (the unlocked vault inside `owd`). Returns `Locked` while the vault
/// is locked and `NoSecret` for an unknown name.
pub trait SecretSource: Send + Sync {
    fn secret(&self, name: &str) -> Result<SecretHandle, ProviderError>;
}

/// What a provider gets for one fetch. Secrets are reachable only through here, and only by
/// the names the provider declared in `Provider::secrets()`.
pub struct FetchCtx<'a> {
    allowed: &'static [&'static str],
    source: Option<&'a dyn SecretSource>,
}

impl<'a> FetchCtx<'a> {
    pub fn new(allowed: &'static [&'static str], source: &'a dyn SecretSource) -> Self {
        FetchCtx {
            allowed,
            source: Some(source),
        }
    }

    /// No secrets at all (local providers, tests).
    pub fn none() -> FetchCtx<'static> {
        FetchCtx {
            allowed: &[],
            source: None,
        }
    }

    pub fn secret(&self, name: &str) -> Result<SecretHandle, ProviderError> {
        if !self.allowed.contains(&name) {
            return Err(ProviderError::NoSecret);
        }
        self.source.ok_or(ProviderError::NoSecret)?.secret(name)
    }
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
        ctx: &FetchCtx<'_>,
    ) -> Result<Map<String, Value>, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Src;
    impl SecretSource for Src {
        fn secret(&self, name: &str) -> Result<SecretHandle, ProviderError> {
            match name {
                "a" | "b" => Ok(SecretHandle::new("FAKE-value")),
                _ => Err(ProviderError::NoSecret),
            }
        }
    }

    #[test]
    fn only_declared_names_resolve() {
        let ctx = FetchCtx::new(&["a"], &Src);
        assert_eq!(ctx.secret("a").unwrap().expose(), "FAKE-value");
        // "b" exists in the source but the provider did not declare it.
        assert_eq!(ctx.secret("b").err(), Some(ProviderError::NoSecret));
        assert_eq!(
            FetchCtx::none().secret("a").err(),
            Some(ProviderError::NoSecret)
        );
    }

    #[test]
    fn handle_debug_is_redacted() {
        let h = SecretHandle::new("FAKE-value");
        assert!(!format!("{h:?}").contains("FAKE"));
    }
}
