//! Provider contract (docs/DAEMON_SPEC.md section 8). Secrets reach a provider only through
//! `FetchCtx` as `SecretHandle`s; network egress is added with the first network provider.
//! Local providers need neither.
use serde_json::{Map, Value};
use std::fmt;
use std::marker::PhantomData;
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

/// A secret lent for the duration of one `fetch` call. The lifetime is the borrow of the
/// `FetchCtx` it came from, so it cannot be stored or returned past the fetch, and only this
/// crate can construct one. Zeroized on drop. Deliberately no `Display`, `Clone`, `Serialize` or
/// `PartialEq`, and `Debug` is redacted, so the value cannot reach a log, an error or a reply by
/// formatting. Prefer `FetchCtx::with_secret`, which never holds the value outside a closure;
/// `expose` is for the future egress wrapper (docs/AUTH_PLAN.md section 3.2).
///
/// The handle cannot be kept past the context it came from, and cannot be built outside this
/// crate:
///
/// ```compile_fail
/// use ow_core::{FetchCtx, SecretHandle};
/// fn keep(ctx: &FetchCtx<'_>) -> SecretHandle<'static> {
///     ctx.secret("a").unwrap()
/// }
/// ```
///
/// ```compile_fail
/// let _ = ow_core::SecretHandle::new("x");
/// ```
pub struct SecretHandle<'a>(Zeroizing<String>, PhantomData<&'a ()>);

impl SecretHandle<'_> {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretHandle<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretHandle(<redacted>)")
    }
}

/// Where secrets come from (the unlocked vault inside `owd`). Lends the value to a closure so
/// the implementor never hands out an owned string. `Locked` while the vault is locked,
/// `NoSecret` for an unknown name.
pub trait SecretSource: Send + Sync {
    fn with_secret(&self, name: &str, f: &mut dyn FnMut(&str)) -> Result<(), ProviderError>;
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

    /// Run `f` with the secret `name`; the value does not outlive the closure.
    pub fn with_secret<R>(
        &self,
        name: &str,
        f: impl FnOnce(&str) -> R,
    ) -> Result<R, ProviderError> {
        if !self.allowed.contains(&name) {
            return Err(ProviderError::NoSecret);
        }
        let source = self.source.ok_or(ProviderError::NoSecret)?;
        let mut f = Some(f);
        let mut out = None;
        source.with_secret(name, &mut |v| {
            if let Some(f) = f.take() {
                out = Some(f(v));
            }
        })?;
        out.ok_or(ProviderError::NoSecret)
    }

    /// A handle for callers that need the value to outlive one closure (the egress wrapper).
    /// It borrows `self`, so it cannot leave the fetch.
    pub fn secret(&self, name: &str) -> Result<SecretHandle<'_>, ProviderError> {
        self.with_secret(name, |v| {
            SecretHandle(Zeroizing::new(v.to_owned()), PhantomData)
        })
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
        fn with_secret(&self, name: &str, f: &mut dyn FnMut(&str)) -> Result<(), ProviderError> {
            match name {
                "a" | "b" => {
                    f("FAKE-value");
                    Ok(())
                }
                _ => Err(ProviderError::NoSecret),
            }
        }
    }

    #[test]
    fn only_declared_names_resolve() {
        let ctx = FetchCtx::new(&["a"], &Src);
        assert_eq!(ctx.secret("a").unwrap().expose(), "FAKE-value");
        assert_eq!(ctx.with_secret("a", |v| v.len()), Ok(10));
        // "b" exists in the source but the provider did not declare it.
        assert_eq!(ctx.secret("b").err(), Some(ProviderError::NoSecret));
        assert_eq!(
            FetchCtx::none().secret("a").err(),
            Some(ProviderError::NoSecret)
        );
    }

    /// True when `$t` implements `$tr` (a probe that compiles either way).
    macro_rules! implements {
        ($t:ty, $tr:path) => {{
            #[allow(dead_code)]
            trait No {
                const V: bool = false;
            }
            struct W<T: ?Sized>(std::marker::PhantomData<T>);
            impl<T: ?Sized> No for W<T> {}
            impl<T: ?Sized + $tr> W<T> {
                #[allow(dead_code)] // unused when the bound does not hold: that is the probe
                const V: bool = true;
            }
            <W<$t>>::V
        }};
    }

    /// The type's doc says a secret handle cannot be formatted, copied, compared or serialized.
    #[test]
    fn secret_handle_cannot_be_displayed_cloned_compared_or_serialized() {
        assert!(implements!(String, std::fmt::Display), "probe is broken");
        assert!(implements!(SecretHandle<'static>, std::fmt::Debug));
        assert!(!implements!(SecretHandle<'static>, std::fmt::Display));
        assert!(!implements!(SecretHandle<'static>, Clone));
        assert!(!implements!(SecretHandle<'static>, Copy));
        assert!(!implements!(SecretHandle<'static>, PartialEq));
        assert!(!implements!(SecretHandle<'static>, serde::Serialize));
    }

    #[test]
    fn handle_debug_is_redacted() {
        let ctx = FetchCtx::new(&["a"], &Src);
        let h = ctx.secret("a").unwrap();
        assert!(!format!("{h:?}").contains("FAKE"));
    }
}
