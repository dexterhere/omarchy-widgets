//! Test-only fake HTTPS server and the only way to point `Egress` at it. This file is compiled
//! under `#[cfg(test)]` and nowhere else: there is no feature flag, no public constructor and
//! no environment variable that can add a root certificate or a plain-http route to a build.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::{Egress, EgressError, Limits};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ureq::Error;
use ureq::config::Config;
use ureq::http::Uri;
use ureq::tls::{Certificate, RootCerts, TlsConfig, TlsProvider};
use ureq::unversioned::resolver::{ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::NextTimeout;

/// One request as the server saw it, byte for byte.
#[derive(Clone)]
pub struct Seen {
    pub request_line: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub raw: Vec<u8>,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

pub enum Reply {
    /// Complete answer with a content-length.
    Full {
        status: u16,
        headers: Vec<(&'static str, String)>,
        body: Vec<u8>,
    },
    /// No content-length; body ends when the connection closes.
    UntilClose { status: u16, body: Vec<u8> },
    /// Promise a large body, then send one byte per `interval` for up to `for_`.
    Drip { interval: Duration, for_: Duration },
    /// Read the request and then say nothing for `for_`.
    Hang { for_: Duration },
}

impl Reply {
    pub fn ok(body: &[u8]) -> Self {
        Self::status(200, body)
    }

    pub fn status(status: u16, body: &[u8]) -> Self {
        Self::Full {
            status,
            headers: Vec::new(),
            body: body.to_vec(),
        }
    }

    pub fn with(status: u16, headers: Vec<(&'static str, String)>, body: &[u8]) -> Self {
        Self::Full {
            status,
            headers,
            body: body.to_vec(),
        }
    }
}

type Handler = Arc<dyn Fn(&Seen) -> Reply + Send + Sync>;

pub struct Fake {
    pub addr: SocketAddr,
    pub cert_der: Vec<u8>,
    pub seen: Arc<Mutex<Vec<Seen>>>,
    pub connections: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
}

impl Fake {
    /// A server whose certificate is valid for `names`.
    pub fn start(names: &[&str], handler: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Self {
        Self::start_with(names, false, handler)
    }

    pub fn start_expired(names: &[&str]) -> Self {
        Self::start_with(names, true, |_| Reply::ok(b"x"))
    }

    fn start_with(
        names: &[&str],
        expired: bool,
        handler: impl Fn(&Seen) -> Reply + Send + Sync + 'static,
    ) -> Self {
        let sans: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
        let key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(sans).unwrap();
        if expired {
            params.not_before = rcgen::date_time_ymd(2000, 1, 1);
            params.not_after = rcgen::date_time_ymd(2001, 1, 1);
        }
        let cert = params.self_signed(&key).unwrap();
        let cert_der = cert.der().to_vec();
        let tls_cfg = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(cert_der.clone())],
            PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key.serialize_der())),
        )
        .unwrap();
        let tls_cfg = Arc::new(tls_cfg);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let connections = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let handler: Handler = Arc::new(handler);
        let (s2, c2, st2) = (seen.clone(), connections.clone(), stop.clone());
        std::thread::spawn(move || {
            for tcp in listener.incoming() {
                if st2.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(tcp) = tcp else { continue };
                c2.fetch_add(1, Ordering::SeqCst);
                let (cfg, h, s) = (tls_cfg.clone(), handler.clone(), s2.clone());
                std::thread::spawn(move || serve(tcp, cfg, h, s));
            }
        });
        Self {
            addr,
            cert_der,
            seen,
            connections,
            stop,
        }
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    pub fn connection_count(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
    }
}

fn serve(
    tcp: TcpStream,
    cfg: Arc<rustls::ServerConfig>,
    handler: Handler,
    seen: Arc<Mutex<Vec<Seen>>>,
) {
    let _ = tcp.set_read_timeout(Some(Duration::from_secs(10)));
    let Ok(conn) = rustls::ServerConnection::new(cfg) else {
        return;
    };
    let mut tls = rustls::StreamOwned::new(conn, tcp);
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    let head_end = loop {
        match tls.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
        }
        if let Some(p) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
    };
    let head = String::from_utf8_lossy(&raw[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("").to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let want: usize = headers
        .iter()
        .find(|(n, _)| n == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    while raw.len() < head_end + want {
        match tls.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
        }
    }
    let request = Seen {
        request_line,
        headers,
        body: raw[head_end..].to_vec(),
        raw,
    };
    seen.lock().unwrap().push(request.clone());
    let head = |status: u16| format!("HTTP/1.1 {status} X\r\nconnection: close\r\n");
    match handler(&request) {
        Reply::Full {
            status,
            headers,
            body,
        } => {
            let mut out = head(status);
            for (n, v) in headers {
                out.push_str(&format!("{n}: {v}\r\n"));
            }
            out.push_str(&format!("content-length: {}\r\n\r\n", body.len()));
            let _ = tls.write_all(out.as_bytes());
            let _ = tls.write_all(&body);
        }
        Reply::UntilClose { status, body } => {
            let _ = tls.write_all(format!("{}\r\n", head(status)).as_bytes());
            let _ = tls.write_all(&body);
        }
        Reply::Drip { interval, for_ } => {
            let _ = tls.write_all(format!("{}content-length: 10000\r\n\r\n", head(200)).as_bytes());
            let start = std::time::Instant::now();
            while start.elapsed() < for_ {
                if tls.write_all(b"x").is_err() || tls.flush().is_err() {
                    return;
                }
                std::thread::sleep(interval);
            }
        }
        Reply::Hang { for_ } => std::thread::sleep(for_),
    }
    let _ = tls.flush();
    tls.conn.send_close_notify();
    let _ = tls.flush();
}

/// Name to local address; the port in the URI (always 443 after validation) is ignored.
#[derive(Debug)]
struct Routes(HashMap<String, SocketAddr>);

impl Resolver for Routes {
    fn resolve(
        &self,
        uri: &Uri,
        _config: &Config,
        _timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, Error> {
        let addr = uri
            .host()
            .and_then(|h| self.0.get(h))
            .ok_or(Error::HostNotFound)?;
        let mut out = self.empty();
        out.push(*addr);
        Ok(out)
    }
}

fn routes(routes: &[(&str, &Fake)]) -> Routes {
    Routes(
        routes
            .iter()
            .map(|(n, f)| ((*n).to_owned(), f.addr))
            .collect(),
    )
}

/// Trusts the certificates of the routed fakes.
pub fn egress(hosts: &[&str], limits: Limits, r: &[(&str, &Fake)]) -> Egress {
    let certs: Vec<Certificate<'static>> = r
        .iter()
        .map(|(_, f)| Certificate::from_der(&f.cert_der).to_owned())
        .collect();
    let tls = TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .root_certs(RootCerts::Specific(Arc::new(certs)))
        .build();
    Egress::for_tests(hosts, limits, tls, routes(r)).unwrap()
}

/// Production roots (bundled Mozilla set), so a self-signed server must be rejected.
pub fn egress_production_roots(
    hosts: &[&str],
    limits: Limits,
    r: &[(&str, &Fake)],
) -> Result<Egress, EgressError> {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .root_certs(RootCerts::WebPki)
        .build();
    Egress::for_tests(hosts, limits, tls, routes(r))
}

/// Resolves every name to a port nobody listens on.
pub fn egress_closed_port(host: &str) -> Egress {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    let tls = TlsConfig::builder().provider(TlsProvider::Rustls).build();
    let r = Routes([(host.to_owned(), addr)].into());
    Egress::for_tests(&[host], Limits::default(), tls, r).unwrap()
}
