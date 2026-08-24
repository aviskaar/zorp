//! A checked endpoint and a resolver that can reach only that endpoint.

use std::fmt;
use std::io;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

#[non_exhaustive]
#[derive(Debug)]
pub enum LoopbackError {
    Malformed { url: String, reason: &'static str },
    Scheme { scheme: String },
    OffDevice { host: String, detail: String },
    Unresolvable { host: String, message: String },
}

impl fmt::Display for LoopbackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoopbackError::Malformed { url, reason } => {
                write!(f, "{url:?} is not a usable voice endpoint: {reason}")
            }
            LoopbackError::Scheme { scheme } => write!(
                f,
                "the {scheme:?} scheme is not a voice endpoint; use http or https on loopback"
            ),
            LoopbackError::OffDevice { host, detail } => write!(
                f,
                "refusing to send recorded audio to {host}: {detail}. Voice is only ever sent to this machine"
            ),
            LoopbackError::Unresolvable { host, message } => {
                write!(f, "cannot resolve {host}: {message}")
            }
        }
    }
}

impl std::error::Error for LoopbackError {}

#[derive(Debug, Clone)]
pub struct LoopbackUrl {
    base: String,
    host: String,
    port: u16,
    addrs: Vec<SocketAddr>,
    direct_runtime: bool,
}

impl LoopbackUrl {
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
        if authority.contains('@') {
            return Err(LoopbackError::Malformed {
                url: raw.to_string(),
                reason: "it carries userinfo, which a local endpoint does not need",
            });
        }

        let (host, port) = split_authority(authority, raw)?;
        let port = port.unwrap_or(if scheme == "https" { 443 } else { 80 });
        let literal = parse_host_literal(&host);
        match literal {
            Some(ip) if !is_loopback(ip) => {
                return Err(LoopbackError::OffDevice {
                    host,
                    detail: "that is not a loopback address".into(),
                });
            }
            None if !host.trim_end_matches('.').eq_ignore_ascii_case("localhost") => {
                return Err(LoopbackError::OffDevice {
                    host,
                    detail: "only a loopback address or `localhost` is accepted".into(),
                });
            }
            _ => {}
        }

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
        if let Some(bad) = addrs.iter().find(|addr| !is_loopback(addr.ip())) {
            return Err(LoopbackError::OffDevice {
                host,
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
            direct_runtime: scheme == "http" && path.is_empty(),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.base
    }

    pub fn host(&self) -> &str {
        self.host.trim_matches(['[', ']'])
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn addrs(&self) -> &[SocketAddr] {
        &self.addrs
    }

    /// Whether `qwen-asr-serve` can bind this URL without an operator proxy.
    pub fn supports_direct_runtime(&self) -> bool {
        self.direct_runtime
    }
}

impl fmt::Display for LoopbackUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.base)
    }
}

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
                    "refusing to connect to {netloc}: voice is pinned to {}:{}",
                    self.host, self.port
                ),
            )
        };
        let Some((host, port)) = netloc.rsplit_once(':') else {
            return Err(refused());
        };
        if port.parse::<u16>().ok() != Some(self.port)
            || host.trim_matches(['[', ']']).to_ascii_lowercase() != self.host
        {
            return Err(refused());
        }
        Ok(self.addrs.clone())
    }
}

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
            value => Some(
                value
                    .strip_prefix(':')
                    .ok_or(malformed("it has junk after the bracket"))?
                    .parse()
                    .map_err(|_| malformed("its port is not a number"))?,
            ),
        };
        return Ok((format!("[{inside}]"), port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => Ok((
            host.to_string(),
            Some(
                port.parse()
                    .map_err(|_| malformed("its port is not a number"))?,
            ),
        )),
        None => Ok((authority.to_string(), None)),
    }
}

fn parse_host_literal(host: &str) -> Option<IpAddr> {
    host.trim_matches(['[', ']']).parse().ok()
}

fn is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map_or_else(|| v6.is_loopback(), |v4| v4.is_loopback()),
    }
}
