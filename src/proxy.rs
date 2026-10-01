use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

use crate::error::{Error, Result};

/// RFC 3986 unreserved characters (`A–Z a–z 0–9 - . _ ~`) are never percent-encoded.
pub(crate) const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Proxy protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    Http,
    Socks5,
}

impl Protocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::Http => "http",
            Protocol::Socks5 => "socks5",
        }
    }
}

/// One generated proxy. The API returns `username:password@host:port` lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proxy {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub protocol: Protocol,
}

impl Proxy {
    /// Parse a `username:password@host:port` line (`@` and `:` are allowed inside the password).
    pub fn parse(line: &str, protocol: Protocol) -> Result<Proxy> {
        let bad = || Error::InvalidProxyLine(line.to_string());
        let (credentials, address) = line.rsplit_once('@').ok_or_else(bad)?;
        let (username, password) = credentials.split_once(':').ok_or_else(bad)?;
        let (host, port) = address.rsplit_once(':').ok_or_else(bad)?;
        let port: u16 = port.parse().map_err(|_| bad())?;
        if username.is_empty() || host.is_empty() || port == 0 {
            return Err(bad());
        }
        Ok(Proxy {
            host: host.into(),
            port,
            username: username.into(),
            password: password.into(),
            protocol,
        })
    }

    /// `http://host:port` — no credentials.
    pub fn server(&self) -> String {
        format!("{}://{}:{}", self.protocol.as_str(), self.host, self.port)
    }

    /// Full URL with percent-encoded credentials, ready for any HTTP client.
    pub fn url(&self) -> String {
        format!(
            "{}://{}:{}@{}:{}",
            self.protocol.as_str(),
            utf8_percent_encode(&self.username, UNRESERVED),
            utf8_percent_encode(&self.password, UNRESERVED),
            self.host,
            self.port
        )
    }

    /// A `reqwest::Proxy` routing all traffic through this proxy.
    pub fn to_reqwest(&self) -> Result<reqwest::Proxy> {
        Ok(reqwest::Proxy::all(self.server())?.basic_auth(&self.username, &self.password))
    }

    /// Environment variables understood by curl, wget, git and most CLIs.
    pub fn env(&self) -> BTreeMap<&'static str, String> {
        [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
        ]
        .into_iter()
        .map(|k| (k, self.url()))
        .collect()
    }

    /// A ready-to-run curl command that prints your exit IP through this proxy.
    pub fn curl(&self) -> String {
        let scheme = if self.protocol == Protocol::Socks5 {
            "socks5h"
        } else {
            "http"
        };
        format!(
            "curl -x '{scheme}://{}:{}@{}:{}' https://api.ipify.org",
            self.username, self.password, self.host, self.port
        )
    }
}

impl fmt::Display for Proxy {
    /// The `username:password@host:port` form the API uses.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}@{}:{}",
            self.username, self.password, self.host, self.port
        )
    }
}

/// The result of a generate call: raw lines plus parsed proxies.
#[derive(Debug, Clone)]
pub struct GeneratedProxies {
    pub lines: Vec<String>,
    pub proxies: Vec<Proxy>,
}

impl GeneratedProxies {
    pub(crate) fn new(lines: Vec<String>, protocol: Protocol) -> Result<Self> {
        let proxies = lines
            .iter()
            .map(|l| Proxy::parse(l, protocol))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { lines, proxies })
    }
}
