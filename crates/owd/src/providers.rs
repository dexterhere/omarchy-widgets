//! Which providers the daemon runs. Local providers always; a network provider only when
//! `config.toml` binds a secret for it (docs/AUTH_PLAN.md 5.2 item 3). The only way a provider
//! learns its secret name is the validated `ProviderBinding`.
use ow_core::{Config, Http, Provider};
use ow_providers::{Battery, GITHUB_HOST, GitHub, System};
use std::sync::Arc;

/// `make_http` builds the transport for a provider from the hosts that provider may contact. It
/// is called only for providers that are configured. Production passes `EgressHttp::new`; tests
/// pass a fake.
pub fn providers(
    config: &Config,
    make_http: impl Fn(&[&str]) -> Result<Arc<dyn Http>, String>,
) -> Result<Vec<Arc<dyn Provider>>, String> {
    let mut out: Vec<Arc<dyn Provider>> = vec![Arc::new(System::new()), Arc::new(Battery::new())];
    if let Some(binding) = config.github() {
        let http = make_http(&[GITHUB_HOST])?;
        out.push(Arc::new(GitHub::new(binding, http)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use ow_core::{Bearer, HttpError, HttpRequest, HttpResponse};
    use std::sync::Mutex;

    struct Never;
    impl Http for Never {
        fn post_json(
            &self,
            _: &HttpRequest<'_>,
            _: &Bearer<'_>,
        ) -> Result<HttpResponse, HttpError> {
            Err(HttpError::Offline)
        }
    }

    fn topics(ps: &[Arc<dyn Provider>]) -> Vec<&'static str> {
        ps.iter()
            .flat_map(|p| p.topics().iter().map(|t| t.name))
            .collect()
    }

    #[test]
    fn github_is_registered_only_when_config_binds_it() {
        let asked = Mutex::new(Vec::<Vec<String>>::new());
        let make = |hosts: &[&str]| {
            asked
                .lock()
                .unwrap()
                .push(hosts.iter().map(|h| (*h).to_owned()).collect());
            Ok(Arc::new(Never) as Arc<dyn Http>)
        };
        let none = providers(&Config::default(), make).unwrap();
        assert!(!topics(&none).contains(&"github.prs"));
        assert!(
            asked.lock().unwrap().is_empty(),
            "no transport without a binding"
        );

        let cfg = Config::parse("[providers.github]\nsecret = \"github-main\"\n").unwrap();
        let some = providers(&cfg, make).unwrap();
        assert!(topics(&some).contains(&"github.prs"));
        assert_eq!(
            *asked.lock().unwrap(),
            vec![vec!["api.github.com".to_owned()]]
        );
        let gh = some.iter().find(|p| p.id() == "github").unwrap();
        assert_eq!(gh.secrets(), ["github-main"]);
        assert_eq!(gh.egress_hosts(), ["api.github.com"]);
        // Local providers are unaffected.
        assert!(topics(&some).contains(&"system.cpu") && topics(&none).contains(&"system.cpu"));
    }

    #[test]
    fn a_transport_that_cannot_be_built_stops_startup() {
        let cfg = Config::parse("[providers.github]\nsecret = \"github-main\"\n").unwrap();
        assert!(providers(&cfg, |_| Err("egress: blocked".into())).is_err());
        // Not configured: the failing builder is never reached.
        assert!(providers(&Config::default(), |_| Err("x".into())).is_ok());
    }
}
