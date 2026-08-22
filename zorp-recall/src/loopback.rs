//! The guard: is this address on the machine the user is sitting at?
//!
//! Everything else in this crate assumes the answer. The corpus being
//! indexed is somebody's entire chat history with an agent that has been
//! reading their files, so an endpoint that turns out to be somewhere else
//! is not a degraded feature, it is the worst thing this code could do.
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

use std::fmt;
use std::io;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

/// Why an endpoint was not accepted as being on this device.
#[non_exhaustive]
#[derive(Debug)]
pub enum LoopbackError {
    /// Not a URL this crate is willing to interpret.
    Malformed { url: String, reason: &'static str },
    /// A scheme other than `http` or `https`.
    Scheme { scheme: String },
    /// The written form is not a loopback address or `localhost`, or the
    /// name resolved to something that is not on this machine.
    OffDevice { host: String, detail: String },
    /// The name could not be resolved at all.
    Unresolvable { host: String, message: String },
}

impl fmt::Display for LoopbackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoopbackError::Malformed { url, reason } => {
                write!(f, "{url:?} is not a usable endpoint: {reason}")
            }
            LoopbackError::Scheme { scheme } => write!(
                f,
                "the {scheme:?} scheme is not an embedding endpoint; use http or https on loopback"
            ),
            LoopbackError::OffDevice { host, detail } => write!(
                f,
                "refusing to embed conversations at {host}: {detail}. \
                 Conversation text is only ever sent to this machine"
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
/// Anything in this crate that opens a socket takes one of these, so "did
/// the guard run" is answered by the type and not by a code review.
#[derive(Debug, Clone)]
pub struct LoopbackUrl {
    base: String,
    host: String,
    port: u16,
    addrs: Vec<SocketAddr>,
}

impl LoopbackUrl {
    /// Check `raw` and keep it, or say why not.
    pub fn parse(raw: &str) -> Result<LoopbackUrl, LoopbackError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(LoopbackError::Malformed {
                url: raw.to_string(),
                reason: "it is empty",
            });
        }
        let (scheme, rest) = raw.split_once("://").ok_or(LoopbackError::Malformed {
            url: raw.to_string(),
            reason: "it has no scheme",
        })?;
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            return Err(LoopbackError::Scheme { scheme });
        }

        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..end];
        let path = rest[end..].trim_end_matches('/');
        if authority.is_empty() {
            return Err(LoopbackError::Malformed {
                url: raw.to_string(),
                reason: "it names no host",
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
            });
        }

        let (host, port) = split_authority(authority, raw)?;
        let port = port.unwrap_or(if scheme == "https" { 443 } else { 80 });

        // Step 1: the written form.
        let literal = parse_host_literal(&host);
        match literal {
            Some(ip) => {
                if !is_loopback(ip) {
                    return Err(LoopbackError::OffDevice {
                        host: host.clone(),
                        detail: "that is not a loopback address".into(),
                    });
                }
            }
            None => {
                // A trailing dot is a fully qualified `localhost.`, which is
                // the same name.
                let name = host.trim_end_matches('.').to_ascii_lowercase();
                if name != "localhost" {
                    return Err(LoopbackError::OffDevice {
                        host: host.clone(),
                        detail: "only a loopback address or `localhost` is accepted".into(),
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
                host: host.clone(),
                message: "it resolved to no addresses".into(),
            });
        }
        // Refused whole, not filtered. A name that answers with one
        // loopback address and one that is not is a name under somebody
        // else's control, and keeping the good half would be trusting it.
        if let Some(bad) = addrs.iter().find(|a| !is_loopback(a.ip())) {
            return Err(LoopbackError::OffDevice {
                host: host.clone(),
                detail: format!("it resolves to {}, which is not on this machine", bad.ip()),
            });
        }

        let authority_text = if literal.is_some_and(|ip| ip.is_ipv6()) {
            format!("[{}]:{port}", host.trim_matches(['[', ']']))
        } else {
            format!("{}:{port}", host.trim_end_matches('.'))
        };
        Ok(LoopbackUrl {
            base: format!("{scheme}://{authority_text}{path}"),
            host,
            port,
            addrs,
        })
    }

    /// The endpoint, normalized, with no trailing slash. Paths are appended
    /// to this.
    pub fn as_str(&self) -> &str {
        &self.base
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

impl fmt::Display for LoopbackUrl {
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
/// this crate can talk to is exactly the set in here.
#[derive(Debug, Clone)]
pub struct LoopbackResolver {
    host: String,
    port: u16,
    addrs: Vec<SocketAddr>,
}

impl LoopbackResolver {
    pub fn for_url(url: &LoopbackUrl) -> LoopbackResolver {
        LoopbackResolver {
            host: url.host().to_ascii_lowercase(),
            port: url.port(),
            addrs: url.addrs().to_vec(),
        }
    }
}

impl ureq::Resolver for LoopbackResolver {
    fn resolve(&self, netloc: &str) -> io::Result<Vec<SocketAddr>> {
        let refused = || {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "refusing to connect to {netloc}: this build only talks to {}:{}, \
                     because conversation text never leaves this machine",
                    self.host, self.port
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
fn split_authority(authority: &str, raw: &str) -> Result<(String, Option<u16>), LoopbackError> {
    let malformed = |reason| LoopbackError::Malformed {
        url: raw.to_string(),
        reason,
    };
    if let Some(rest) = authority.strip_prefix('[') {
        let (inside, after) = rest
            .split_once(']')
            .ok_or(malformed("its bracket is unclosed"))?;
        let port = match after {
            "" => None,
            p => Some(
                p.strip_prefix(':')
                    .ok_or(malformed("it has junk after the bracket"))?
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
