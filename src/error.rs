use std::time::Duration;

/// Everything that can go wrong when calling BuyProxy.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The API answered with a non-2xx status.
    #[error("{message} (HTTP {status})")]
    Api {
        status: u16,
        /// The API's own error text.
        message: String,
        /// Raw response body.
        body: String,
        /// Set on 429 when the API said how long to wait.
        retry_after: Option<Duration>,
    },
    /// No API key was configured for an endpoint that needs one.
    #[error("no API key — use ClientBuilder::api_key or set BUYPROXY_API_KEY (create one at https://www.buyproxy.org/dashboard/api-keys)")]
    MissingApiKey,
    /// The request never got a response (DNS, reset, timeout, offline).
    #[error("could not reach BuyProxy: {0}")]
    Connection(#[from] reqwest::Error),
    /// The response wasn't the JSON we expected.
    #[error("unexpected response from BuyProxy: {0}")]
    Decode(String),
    /// A line from the API wasn't `username:password@host:port`.
    #[error("not a proxy line (expected username:password@host:port): {0:?}")]
    InvalidProxyLine(String),
    /// A payment was still pending when `transactions().wait` gave up.
    #[error("payment {0} is still pending")]
    StillPending(String),
}

impl Error {
    fn status(&self) -> Option<u16> {
        match self {
            Error::Api { status, .. } => Some(*status),
            Error::MissingApiKey => Some(401),
            _ => None,
        }
    }
    /// 400 — a parameter was invalid.
    pub fn is_validation(&self) -> bool {
        self.status() == Some(400)
    }
    /// 401 — the key is missing or invalid.
    pub fn is_authentication(&self) -> bool {
        self.status() == Some(401)
    }
    /// 402 — empty reseller pool, or a plan with no GB left.
    pub fn is_insufficient_bandwidth(&self) -> bool {
        self.status() == Some(402)
    }
    /// 403 — suspended account/customer, or the plan doesn't include this endpoint.
    pub fn is_forbidden(&self) -> bool {
        self.status() == Some(403)
    }
    /// 404 — the plan, customer or transaction doesn't exist on this account.
    pub fn is_not_found(&self) -> bool {
        self.status() == Some(404)
    }
    /// 429 — more than 60 requests/minute on this key.
    pub fn is_rate_limited(&self) -> bool {
        self.status() == Some(429)
    }
    /// 5xx — usually safe to retry.
    pub fn is_server_error(&self) -> bool {
        self.status().is_some_and(|s| s >= 500)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
