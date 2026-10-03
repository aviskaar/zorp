//! The guard: is this address on the machine the user is sitting at?
//!
//! Everything that uses this crate assumes the answer. `zorp-recall` sends
//! it somebody's entire chat history with an agent that has been reading
//! their files, and `zorp-voice` sends it a recording of their voice, so an
//! endpoint that turns out to be somewhere else is not a degraded feature,
//! it is the worst thing either could do.
//!
//! Three checks, in this order, and all three have to pass.
//!
//! 1. The written form. The host is a loopback IP literal, or it is exactly
//!    `localhost`. A substring test for "127.0.0.1" would accept
//!    `http://127.0.0.1.evil.example`, which is a name somebody else owns.
//! 2. The resolution. The name is resolved once, here, and every address it
//!    yields has to be loopback. This is what catches a `localhost` pointed
//!    somewhere else in `/etc/hosts`.
//! 3. The connection. The addresses from step 2 are kept, and
//!    `LoopbackResolver` is the only thing the HTTP client is allowed to
//!    resolve through. It does no lookup of its own and answers for exactly
//!    one host and port, so a redirect, a proxy, or anything else that asks
//!    for a different destination gets an error instead of a socket.
//!
//! Step 3 is why the addresses are stored rather than re-derived. Checking
//! a name and then letting the client look it up again is a check with a
//! gap in the middle, and the gap is where the answer changes.
//!
//! What is not here, on purpose: `redirects(0)` and
//! `try_proxy_from_env(false)`. Those are settings on the HTTP agent, and
//! each caller builds its own agent next to the request it guards, so the
//! four layers can be read in one place in each crate. Nor is the wording
//! of a refusal: the caller knows what it is refusing to send, and says so
//! through [`Wording`].

use std::fmt;
use std::io;
use std::marker::PhantomData;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

/// The caller's half of every sentence this crate writes.
///
/// A refusal has to say what is being protected, and only the caller knows
/// that, so each field fills a fixed slot in one fixed sentence. The
/// sentences themselves, and every decision about when to use them, stay
/// here.
#[derive(Debug)]
pub struct Phrases {
    /// `"{url:?} is not a usable {endpoint}: {reason}"`
    pub endpoint: &'static str,
    /// `"the {scheme:?} scheme is not {not_scheme_for}; use http or https on loopback"`
    pub not_scheme_for: &'static str,
    /// `"refusing to {refusing_to} {host}: {detail}. {only_ever}"`
    pub refusing_to: &'static str,
    /// The closing sentence of the same refusal.
    pub only_ever: &'static str,
    /// `"refusing to connect to {netloc}: {pinned_before}{host}:{port}{pinned_after}"`,
    /// the resolver's refusal.
    pub pinned_before: &'static str,
    /// The end of the same refusal, after the host and port.
    pub pinned_after: &'static str,
}

/// Names the [`Phrases`] a caller's refusals are written in. Implemented
/// by a unit type in each caller, which is also what keeps one crate's
/// checked endpoint from being handed to another crate's client.
pub trait Wording: 'static {
    const PHRASES: &'static Phrases;
}

/// Why an endpoint was not accepted as being on this device.
#[non_exhaustive]
#[derive(Debug)]
pub enum LoopbackError {
    /// Not a URL this crate is willing to interpret.
    Malformed {
        url: String,
        reason: &'static str,
        wording: &'static Phrases,
    },
    /// A scheme other than `http` or `https`.
    Scheme {
        scheme: String,
        wording: &'static Phrases,
    },
    /// The written form is not a loopback address or `localhost`, or the
    /// name resolved to something that is not on this machine.
    OffDevice {
        host: String,
        detail: String,
        wording: &'static Phrases,
    },
    /// The name could not be resolved at all.
    Unresolvable { host: String, message: String },
}

impl fmt::Display for LoopbackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoopbackError::Malformed {
                url,
                reason,
                wording,
            } => {
                write!(f, "{url:?} is not a usable {}: {reason}", wording.endpoint)
            }
            LoopbackError::Scheme { scheme, wording } => write!(
                f,
                "the {scheme:?} scheme is not {}; use http or https on loopback",
                wording.not_scheme_for
            ),
            LoopbackError::OffDevice {
                host,
                detail,
                wording,
            } => write!(
                f,
                "refusing to {} {host}: {detail}. {}",
                wording.refusing_to, wording.only_ever
            ),
            LoopbackError::Unresolvable { host, message } => {
                write!(f, "cannot resolve {host}: {message}")
            }
        }
    }
}

impl std::error::Error for LoopbackError {}

/// An endpoint that has been checked and found to be on this device, with
/// the addresses it resolved to at the time of checking.
///
/// There is no way to build one except through `parse`, which is the point.
/// Anything that opens a socket takes one of these, so "did the guard run"
/// is answered by the type and not by a code review.
#[derive(Debug, Clone)]
pub struct LoopbackUrl<W> {
    base: String,
    scheme: String,
    path: String,
    host: String,
    port: u16,
    addrs: Vec<SocketAddr>,
    wording: PhantomData<fn() -> W>,
}

impl<W: Wording> LoopbackUrl<W> {
    /// Check `raw` and keep it, or say why not.
    pub fn parse(raw: &str) -> Result<LoopbackUrl<W>, LoopbackError> {
        let wording = W::PHRASES;
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(LoopbackError::Malformed {
                url: raw.to_string(),
                reason: "it is empty",
                wording,
            });
        }
        let (scheme, rest) = raw
            .split_once("://")
            .ok_or_else(|| LoopbackError::Malformed {
                url: raw.to_string(),
                reason: "it has no scheme",
                wording,
            })?;
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return Err(LoopbackError::Scheme { scheme, wording });
        }

        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..end];
        let path = rest[end..].trim_end_matches('/');
        if authority.is_empty() {
            return Err(LoopbackError::Malformed {
                url: raw.to_string(),
                reason: "it names no host",
                wording,
            });
        }
        // Userinfo is refused outright rather than parsed past. It is not
        // needed for a local endpoint, and `http://127.0.0.1@evil.example/`
        // is a URL whose host is `evil.example` while it reads like
        // loopback. Refusing the whole shape is simpler than being right
        // about it every time.
        if authority.contains('@') {
            return Err(LoopbackError::Malformed {
                url: raw.to_string(),
                reason: "it carries userinfo, which a local endpoint does not need",
                wording,
            });
        }

        let (host, port) = split_authority(authority, raw, wording)?;
        let port = port.unwrap_or(if scheme == "https" { 443 } else { 80 });

        // Step 1: the written form.
        let literal = parse_host_literal(&host);
        match literal {
            Some(ip) => {
                if !is_loopback(ip) {
                    return Err(LoopbackError::OffDevice {
                        host,
                        detail: "that is not a loopback address".into(),
                        wording,
                    });
                }
            }
            None => {
                // A trailing dot is a fully qualified `localhost.`, which is
                // the same name.
                let name = host.trim_end_matches('.').to_ascii_lowercase();
                if name != "localhost" {
                    return Err(LoopbackError::OffDevice {
                        host,
                        detail: "only a loopback address or `localhost` is accepted".into(),
                        wording,
                    });
                }
            }
        }

        // Step 2: the resolution.
        let netloc = format!("{}:{port}", host.trim_end_matches('.'));
        let addrs: Vec<SocketAddr> = netloc
            .to_socket_addrs()
            .map_err(|e| LoopbackError::Unresolvable {
                host: host.clone(),
                message: e.to_string(),
            })?
            .collect();
        if addrs.is_empty() {
            return Err(LoopbackError::Unresolvable {
                host,
                message: "it resolved to no addresses".into(),
            });
        }
        // Refused whole, not filtered. A name that answers with one
        // loopback address and one that is not is a name under somebody
        // else's control, and keeping the good half would be trusting it.
        if let Some(bad) = addrs.iter().find(|a| !is_loopback(a.ip())) {
            return Err(LoopbackError::OffDevice {
                host,
                detail: format!("it resolves to {}, which is not on this machine", bad.ip()),
                wording,
            });
        }

        let authority_text = if literal.is_some_and(|ip| ip.is_ipv6()) {
            format!("[{}]:{port}", host.trim_matches(['[', ']']))
        } else {
            format!("{}:{port}", host.trim_end_matches('.'))
        };
        Ok(LoopbackUrl {
            base: format!("{scheme}://{authority_text}{path}"),
            scheme,
            path: path.to_string(),
            host,
            port,
            addrs,
            wording: PhantomData,
        })
    }
}

impl<W> LoopbackUrl<W> {
    /// The endpoint, normalized, with no trailing slash. Paths are appended
    /// to this.
    pub fn as_str(&self) -> &str {
        &self.base
    }

    /// `http` or `https`, lowercased. Nothing else gets past `parse`.
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// Everything after the authority as written, query and fragment
    /// included, with trailing slashes removed. Empty for a bare host.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The host as written, without brackets around an IPv6 literal.
    pub fn host(&self) -> &str {
        self.host.trim_matches(['[', ']'])
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Every address this endpoint resolved to when it was checked. All
    /// loopback, or `parse` would have refused.
    pub fn addrs(&self) -> &[SocketAddr] {
        &self.addrs
    }
}

impl<W> fmt::Display for LoopbackUrl<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.base)
    }
}

/// The DNS resolver the HTTP client is given, and the reason a redirect or
/// a proxy cannot move the request.
///
/// It performs no lookup. It knows one host and port, answers with the
/// addresses `LoopbackUrl::parse` already validated, and returns an error
/// for anything else it is asked about. `ureq` routes every connection,
/// including a proxied one, through the resolver, so the set of machines
/// a caller can talk to is exactly the set in here.
#[derive(Debug, Clone)]
pub struct LoopbackResolver<W> {
    host: String,
    port: u16,
    addrs: Vec<SocketAddr>,
    wording: PhantomData<fn() -> W>,
}

impl<W> LoopbackResolver<W> {
    pub fn for_url(url: &LoopbackUrl<W>) -> LoopbackResolver<W> {
        LoopbackResolver {
            host: url.host().to_ascii_lowercase(),
            port: url.port(),
            addrs: url.addrs().to_vec(),
            wording: PhantomData,
        }
    }
}

impl<W: Wording> ureq::Resolver for LoopbackResolver<W> {
    fn resolve(&self, netloc: &str) -> io::Result<Vec<SocketAddr>> {
        let refused = || {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "refusing to connect to {netloc}: {}{}:{}{}",
                    W::PHRASES.pinned_before,
                    self.host,
                    self.port,
                    W::PHRASES.pinned_after
                ),
            )
        };
        let Some((host, port)) = netloc.rsplit_once(':') else {
            return Err(refused());
        };
        if port.parse::<u16>().ok() != Some(self.port) {
            return Err(refused());
        }
        if host.trim_matches(['[', ']']).to_ascii_lowercase() != self.host {
            return Err(refused());
        }
        Ok(self.addrs.clone())
    }
}

/// Split `host:port`, handling a bracketed IPv6 literal. An unparseable
/// port is a refusal, not a fallback to the default: a URL nobody can read
/// the same way twice is not a URL to guess about.
fn split_authority(
    authority: &str,
    raw: &str,
    wording: &'static Phrases,
) -> Result<(String, Option<u16>), LoopbackError> {
    let malformed = |reason| LoopbackError::Malformed {
        url: raw.to_string(),
        reason,
        wording,
    };
    if let Some(rest) = authority.strip_prefix('[') {
        let (inside, after) = rest
            .split_once(']')
            .ok_or_else(|| malformed("its bracket is unclosed"))?;
        let port = match after {
            "" => None,
            p => Some(
                p.strip_prefix(':')
                    .ok_or_else(|| malformed("it has junk after the bracket"))?
                    .parse()
                    .map_err(|_| malformed("its port is not a number"))?,
            ),
        };
        return Ok((format!("[{inside}]"), port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => {
            let port = port
                .parse()
                .map_err(|_| malformed("its port is not a number"))?;
            Ok((host.to_string(), Some(port)))
        }
        None => Ok((authority.to_string(), None)),
    }
}

/// The host as an IP address, if it is written as one. `[::1]` counts.
fn parse_host_literal(host: &str) -> Option<IpAddr> {
    let inner = host.trim_matches(['[', ']']);
    inner.parse::<IpAddr>().ok()
}

/// Loopback, after unmapping. `Ipv6Addr::is_loopback` says no to
/// `::ffff:127.0.0.1`, which is an IPv4 loopback address wearing an IPv6
/// hat and reaches exactly the same place.
fn is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.is_loopback(),
            None => v6.is_loopback(),
        },
    }
}
