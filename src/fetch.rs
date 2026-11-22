//! `knowledge fetch <url>` (Stage 4, D7): one page into the capture store, answered within the
//! budget the way `keep` answers. Safety lives here, not in a hook:
//! - http and https only, no proxy from the environment;
//! - every address the host resolves to is checked, and one private, loopback, link-local,
//!   unique-local, CGNAT, multicast or unspecified address refuses the host (IPv4 inside IPv6
//!   included), so a name with one public and one private record cannot slip through;
//! - the connection goes to the addresses that were checked (`Pinned`), so DNS cannot change
//!   between the check and the connect;
//! - redirects are followed here, at most 5, each one checked again;
//! - 20 s for the whole call, 5 MB of body.
use crate::capture::{self, BUDGET};
use crate::config;
use crate::{Fault, emit};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ureq::http::Uri;
use ureq::tls::{RootCerts, TlsConfig};
use ureq::unversioned::resolver::{ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

const MAX_REDIRECTS: usize = 5;
const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_BODY: u64 = 5 * 1024 * 1024;

fn refused4(a: Ipv4Addr) -> Option<&'static str> {
    let o = a.octets();
    Some(if a.is_loopback() {
        "loopback"
    } else if a.is_private() {
        "private (RFC 1918)"
    } else if a.is_link_local() {
        "link-local"
    } else if o[0] == 100 && o[1] & 0xc0 == 64 {
        "CGNAT (100.64/10)"
    } else if a.is_multicast() || a.is_broadcast() {
        "multicast"
    } else if o[0] == 0 {
        "unspecified"
    } else {
        return None;
    })
}

/// Why an address is refused, or None when it is public.
pub fn refused(ip: IpAddr) -> Option<&'static str> {
    match ip {
        IpAddr::V4(a) => refused4(a),
        IpAddr::V6(a) => {
            let s = a.segments();
            if let Some(v4) = a.to_ipv4_mapped() {
                return refused4(v4);
            }
            // NAT64 (64:ff9b::/96) and 6to4 (2002::/16) carry an IPv4 address inside.
            let inner = |hi: u16, lo: u16| Ipv4Addr::from(((hi as u32) << 16) | lo as u32);
            if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                return refused4(inner(s[6], s[7]));
            }
            if s[0] == 0x2002 {
                return refused4(inner(s[1], s[2]));
            }
            Some(if a.is_loopback() {
                "loopback"
            } else if a.is_unspecified() || s[..6] == [0; 6] {
                "unspecified"
            } else if s[0] & 0xfe00 == 0xfc00 {
                "unique-local (fc00::/7)"
            } else if s[0] & 0xffc0 == 0xfe80 {
                "link-local"
            } else if a.is_multicast() {
                "multicast"
            } else {
                return None;
            })
        }
    }
}

fn host_port(uri: &Uri) -> Result<(String, u16), Fault> {
    let bad = || Fault::Net(format!("{uri}: not an http or https URL"));
    let port = match uri.scheme_str() {
        Some("http") => 80,
        Some("https") => 443,
        _ => return Err(bad()),
    };
    let auth = uri.authority().ok_or_else(bad)?;
    if auth.as_str().contains('@') {
        return Err(Fault::Net(format!(
            "{uri}: credentials in a URL are refused"
        )));
    }
    Ok((auth.host().to_string(), auth.port_u16().unwrap_or(port)))
}

/// Resolve and check one URL's host. The addresses returned are the only ones connected to.
fn resolve(uri: &Uri, allow_private: bool) -> Result<Vec<SocketAddr>, Fault> {
    let (host, port) = host_port(uri)?;
    let addrs: Vec<SocketAddr> = format!("{host}:{port}")
        .to_socket_addrs()
        .map_err(|e| Fault::Net(format!("{host}: {e}")))?
        .take(16)
        .collect();
    if addrs.is_empty() {
        return Err(Fault::Net(format!("{host}: no address")));
    }
    if !allow_private {
        for a in &addrs {
            if let Some(why) = refused(a.ip()) {
                return Err(Fault::Net(format!(
                    "refused: {host} resolves to {} ({why}); --allow-private lifts this",
                    a.ip()
                )));
            }
        }
    }
    Ok(addrs)
}

/// Hands ureq the addresses `resolve` checked, and nothing else.
#[derive(Debug)]
struct Pinned {
    host: String,
    addrs: Vec<SocketAddr>,
}

impl Resolver for Pinned {
    fn resolve(
        &self,
        uri: &Uri,
        _config: &ureq::config::Config,
        _timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        if uri.host() != Some(self.host.as_str()) {
            return Err(ureq::Error::HostNotFound);
        }
        let mut out = self.empty();
        for a in &self.addrs {
            out.push(*a);
        }
        Ok(out)
    }
}

fn agent(host: String, addrs: Vec<SocketAddr>, left: Duration) -> ureq::Agent {
    let tls = TlsConfig::builder()
        .root_certs(RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    let config = ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(left))
        .user_agent("knowledge-fetch")
        .tls_config(tls)
        .build();
    ureq::Agent::with_parts(config, DefaultConnector::new(), Pinned { host, addrs })
}

/// A redirect's target, resolved against the URL it came from.
fn join(base: &Uri, location: &str) -> Result<Uri, Fault> {
    let bad = |e: String| Fault::Net(format!("redirect to {location}: {e}"));
    if location.contains("://") {
        return location
            .parse()
            .map_err(|e: ureq::http::uri::InvalidUri| bad(e.to_string()));
    }
    let scheme = base.scheme_str().unwrap_or("https");
    let auth = base.authority().map_or("", |a| a.as_str());
    let path = if let Some(rest) = location.strip_prefix("//") {
        return format!("{scheme}://{rest}")
            .parse()
            .map_err(|e: ureq::http::uri::InvalidUri| bad(e.to_string()));
    } else if location.starts_with('/') {
        location.to_string()
    } else {
        let dir = base.path().rsplit_once('/').map_or("", |(d, _)| d);
        format!("{dir}/{location}")
    };
    format!("{scheme}://{auth}{path}")
        .parse()
        .map_err(|e: ureq::http::uri::InvalidUri| bad(e.to_string()))
}

/// The page's text: HTML converted to Markdown, other text and JSON as they are.
fn text_of(content_type: &str, body: &[u8]) -> Result<String, Fault> {
    let ct = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let raw = String::from_utf8_lossy(body);
    if ct == "text/html" || ct == "application/xhtml+xml" {
        // A link keeps its text and drops its URL, an image goes: on a docs page the URLs were
        // most of the tokens (one playground link ran to 600 chars) and the agent reads, not clicks.
        let md = htmd::HtmlToMarkdown::builder()
            .skip_tags(vec![
                "script", "style", "noscript", "svg", "nav", "footer", "iframe", "form", "img",
            ])
            .add_handler(
                vec!["a"],
                |h: &dyn htmd::element_handler::Handlers, el: &htmd::Element| {
                    Some(h.walk_children_content(el.node, el.context).into())
                },
            )
            .build()
            .convert(&raw)
            .map_err(|e| Fault::Net(format!("converting the page: {e}")))?;
        Ok(md)
    } else if ct.starts_with("text/") || ct == "application/json" || ct.ends_with("+json") {
        Ok(raw.replace("\r\n", "\n"))
    } else {
        Err(Fault::Net(format!(
            "refused: {ct} is not text, HTML or JSON"
        )))
    }
}

/// GET with the checks above. Returns the final URL, its content type and the body.
pub fn get(url: &str, allow_private: bool) -> Result<(String, String, Vec<u8>), Fault> {
    let start = Instant::now();
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err(Fault::Net(format!("{url}: not an http or https URL")));
    }
    let mut uri: Uri = url
        .parse()
        .map_err(|e: ureq::http::uri::InvalidUri| Fault::Net(format!("{url}: {e}")))?;
    for _ in 0..=MAX_REDIRECTS {
        let addrs = resolve(&uri, allow_private)?;
        let host = uri.host().unwrap_or_default().to_string();
        let left = TIMEOUT
            .checked_sub(start.elapsed())
            .ok_or_else(|| Fault::Net(format!("{url}: timed out")))?;
        let mut resp = agent(host, addrs, left)
            .get(uri.clone())
            .call()
            .map_err(|e| Fault::Net(format!("{uri}: {e}")))?;
        let status = resp.status();
        if status.is_redirection()
            && let Some(loc) = resp.headers().get("location").and_then(|v| v.to_str().ok())
        {
            uri = join(&uri, loc)?;
            continue;
        }
        if !status.is_success() {
            return Err(Fault::Net(format!("{uri}: HTTP {}", status.as_u16())));
        }
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/plain")
            .to_string();
        let body = resp
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_to_vec()
            .map_err(|e| Fault::Net(format!("{uri}: {e}")))?;
        return Ok((uri.to_string(), ct, body));
    }
    Err(Fault::Net(format!(
        "{url}: more than {MAX_REDIRECTS} redirects"
    )))
}

/// `knowledge fetch <url> [--q "<q>"]... [--budget N] [--allow-private]`
pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let (args, queries) = capture::split_args(raw, &["budget"])?;
    let Some(url) = args.words.first() else {
        return Err(Fault::Usage(
            "knowledge fetch <url> [--q \"<q>\"]... [--budget N] [--allow-private]".into(),
        ));
    };
    let (_, ct, body) = get(url, args.has("allow-private"))?;
    let text = text_of(&ct, &body)?;
    let cfg = config::load()?;
    let label = format!("fetch:{url}");
    let (id, secs) = capture::store(&cfg, &label, &text)?;
    let budget = args.num("budget", BUDGET)?.max(200);
    emit(&capture::print(
        &cfg, id, &label, &text, &secs, &queries, budget,
    )?);
    Ok(0)
}
