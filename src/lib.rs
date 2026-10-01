//! # BuyProxy
//!
//! Official Rust SDK for [BuyProxy](https://www.buyproxy.org) — residential, mobile, ISP and
//! datacenter proxies. Async, fully typed, retries rate limits for you.
//!
//! ```no_run
//! use buyproxy::{Client, GenerateParams};
//!
//! # async fn run() -> buyproxy::Result<()> {
//! let client = Client::new(); // reads BUYPROXY_API_KEY
//! let plans = client.plans().list().await?;
//! let out = client.plans().generate(&plans[0].id, &GenerateParams::new().country("US").count(5)).await?;
//! let http = reqwest::Client::builder().proxy(out.proxies[0].to_reqwest()?).build()?;
//! println!("{}", http.get("https://api.ipify.org").send().await?.text().await?);
//! # Ok(()) }
//! ```

mod error;
pub mod models;
mod proxy;

use std::time::Duration;

use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::Serialize;

pub use error::{Error, Result};
pub use models::*;
pub use proxy::{GeneratedProxies, Protocol, Proxy};

/// SDK version, sent in the User-Agent.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The production API.
pub const DEFAULT_BASE_URL: &str = "https://www.buyproxy.org";

/// Builds a [`Client`].
#[derive(Debug, Clone)]
pub struct ClientBuilder {
    api_key: Option<String>,
    base_url: String,
    timeout: Duration,
    max_retries: u32,
    http: Option<reqwest::Client>,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            api_key: std::env::var("BUYPROXY_API_KEY")
                .ok()
                .filter(|k| !k.is_empty()),
            base_url: DEFAULT_BASE_URL.into(),
            timeout: Duration::from_secs(30),
            max_retries: 2,
            http: None,
        }
    }
}

impl ClientBuilder {
    /// `bp_key_…` (personal) or `bp_live_…` (reseller). Defaults to `$BUYPROXY_API_KEY`.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into()).filter(|k| !k.is_empty());
        self
    }
    /// Override the API origin (default `https://www.buyproxy.org`).
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').into();
        self
    }
    /// Per-request timeout (default 30 s).
    pub fn timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }
    /// Retries for rate limits and transient failures (default 2).
    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }
    /// Bring your own `reqwest::Client`.
    pub fn http_client(mut self, c: reqwest::Client) -> Self {
        self.http = Some(c);
        self
    }

    pub fn build(self) -> Client {
        let http = self.http.unwrap_or_else(|| {
            reqwest::Client::builder()
                .timeout(self.timeout)
                .build()
                .expect("reqwest client")
        });
        Client {
            api_key: self.api_key,
            base_url: self.base_url,
            max_retries: self.max_retries,
            http,
        }
    }
}

struct Req<'a> {
    method: Method,
    path: String,
    query: Vec<(&'a str, String)>,
    body: Option<serde_json::Value>,
    idempotency_key: Option<String>,
    public: bool,
    retry_5xx: bool,
}

impl<'a> Req<'a> {
    fn get(path: impl Into<String>) -> Self {
        Self {
            method: Method::GET,
            path: path.into(),
            query: vec![],
            body: None,
            idempotency_key: None,
            public: false,
            retry_5xx: true,
        }
    }
    fn post(path: impl Into<String>, body: &impl Serialize) -> Self {
        Self {
            method: Method::POST,
            path: path.into(),
            query: vec![],
            body: Some(serde_json::to_value(body).expect("serializable body")),
            idempotency_key: None,
            public: false,
            retry_5xx: false,
        }
    }
    fn public(mut self) -> Self {
        self.public = true;
        self
    }
}

/// BuyProxy API client. Cheap to clone and safe to share between tasks.
#[derive(Debug, Clone)]
pub struct Client {
    api_key: Option<String>,
    base_url: String,
    max_retries: u32,
    http: reqwest::Client,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    /// A client configured from `BUYPROXY_API_KEY`.
    pub fn new() -> Self {
        ClientBuilder::default().build()
    }
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// `"reseller"` for `bp_live_` keys, `"personal"` for `bp_key_` keys, `None` with no key.
    pub fn key_kind(&self) -> Option<&'static str> {
        self.api_key.as_deref().map(|k| {
            if k.starts_with("bp_live_") {
                "reseller"
            } else {
                "personal"
            }
        })
    }

    async fn send<T: DeserializeOwned>(&self, req: Req<'_>) -> Result<T> {
        let key = match (&self.api_key, req.public) {
            (None, false) => return Err(Error::MissingApiKey),
            (k, _) => k.clone(),
        };
        let url = format!("{}{}", self.base_url, req.path);

        let mut attempt = 0;
        loop {
            let mut rb = self
                .http
                .request(req.method.clone(), &url)
                .header("Accept", "application/json")
                .header("User-Agent", format!("buyproxy-rust/{VERSION}"));
            if let Some(k) = &key {
                rb = rb.header("X-API-Key", k);
            }
            if !req.query.is_empty() {
                rb = rb.query(&req.query);
            }
            if let Some(k) = &req.idempotency_key {
                rb = rb.header("Idempotency-Key", k);
            }
            if let Some(b) = &req.body {
                rb = rb.json(b);
            }

            let response = match rb.send().await {
                Ok(r) => r,
                Err(e) if req.retry_5xx && attempt < self.max_retries => {
                    tokio::time::sleep(backoff(attempt)).await;
                    attempt += 1;
                    let _ = e;
                    continue;
                }
                Err(e) => return Err(Error::Connection(e)),
            };

            let status = response.status();
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|s| *s >= 0.0)
                .map(|s| Duration::from_secs_f64(s.min(30.0)));
            let text = response.text().await?;

            if status.is_success() {
                let text = if text.is_empty() {
                    "null".to_string()
                } else {
                    text
                };
                return serde_json::from_str(&text).map_err(|e| Error::Decode(e.to_string()));
            }

            let wait = if attempt >= self.max_retries {
                None
            } else if status.as_u16() == 429 {
                Some(retry_after.unwrap_or_else(|| backoff(attempt)))
            } else if req.retry_5xx && matches!(status.as_u16(), 502..=504) {
                Some(backoff(attempt))
            } else {
                None
            };
            if let Some(w) = wait {
                tokio::time::sleep(w).await;
                attempt += 1;
                continue;
            }

            let message = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str().map(String::from)))
                .unwrap_or_else(|| "BuyProxy API error".into());
            return Err(Error::Api {
                status: status.as_u16(),
                message,
                body: text,
                retry_after,
            });
        }
    }

    // --- Public ---------------------------------------------------------------

    /// Every product with its key, minimum GB and live price. No API key needed.
    pub async fn products(&self) -> Result<Catalog> {
        self.send(Req::get("/v1/products").public()).await
    }

    /// Create an account without a browser: [`Client::signup`], then [`Client::verify_otp`].
    pub async fn signup(
        &self,
        email: &str,
        password: &str,
        name: Option<&str>,
    ) -> Result<SignupResult> {
        let body = serde_json::json!({ "email": email, "password": password, "name": name })
            .as_object()
            .map(|o| {
                o.iter()
                    .filter(|(_, v)| !v.is_null())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<serde_json::Map<_, _>>()
            })
            .unwrap_or_default();
        self.send(Req::post("/api/auth/signup", &body).public())
            .await
    }
    pub async fn verify_otp(&self, email: &str, code: &str) -> Result<VerifyResult> {
        self.send(
            Req::post(
                "/api/auth/verify-otp",
                &serde_json::json!({ "email": email, "code": code }),
            )
            .public(),
        )
        .await
    }
    pub async fn resend_otp(&self, email: &str) -> Result<()> {
        let _: serde_json::Value = self
            .send(
                Req::post(
                    "/api/auth/resend-otp",
                    &serde_json::json!({ "email": email }),
                )
                .public(),
            )
            .await?;
        Ok(())
    }

    // --- Personal API -----------------------------------------------------------

    pub async fn account(&self) -> Result<Account> {
        self.send(Req::get("/v1/account")).await
    }

    /// Spendable GB per product, including GB still being added after a payment.
    pub async fn balances(&self) -> Result<Vec<Balance>> {
        #[derive(serde::Deserialize)]
        struct R {
            balances: Vec<Balance>,
        }
        Ok(self.send::<R>(Req::get("/v1/balances")).await?.balances)
    }

    pub fn plans(&self) -> Plans<'_> {
        Plans(self)
    }
    pub fn topup(&self) -> Topup<'_> {
        Topup(self)
    }
    pub fn transactions(&self) -> Transactions<'_> {
        Transactions(self)
    }
    pub fn reseller(&self) -> Reseller<'_> {
        Reseller(self)
    }
}

fn backoff(attempt: u32) -> Duration {
    let base = (500.0 * 2f64.powi(attempt as i32)).min(8000.0);
    let jitter = 0.75
        + (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
            % 500) as f64
            / 1000.0;
    Duration::from_millis((base * jitter) as u64)
}

fn seg(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, proxy::UNRESERVED).to_string()
}

fn generate_body(params: &GenerateParams, extra: Option<(&str, &str)>) -> serde_json::Value {
    let mut v = serde_json::to_value(params).expect("serializable");
    if let (Some((k, val)), Some(obj)) = (extra, v.as_object_mut()) {
        obj.insert(k.into(), val.into());
    }
    v
}

/// Plans and proxy generation.
pub struct Plans<'a>(&'a Client);
impl Plans<'_> {
    pub async fn list(&self) -> Result<Vec<Plan>> {
        #[derive(serde::Deserialize)]
        struct R {
            plans: Vec<Plan>,
        }
        Ok(self.0.send::<R>(Req::get("/v1/plans")).await?.plans)
    }

    /// Generate proxy lines against one of your plans. Lines are on-demand — don't store them.
    pub async fn generate(
        &self,
        plan_id: &str,
        params: &GenerateParams,
    ) -> Result<GeneratedProxies> {
        #[derive(serde::Deserialize)]
        struct R {
            lines: Vec<String>,
        }
        let r: R = self
            .0
            .send(Req::post(
                format!("/v1/plans/{}/generate", seg(plan_id)),
                &generate_body(params, None),
            ))
            .await?;
        GeneratedProxies::new(r.lines, params.protocol.unwrap_or_default())
    }
}

/// Card and crypto top-ups.
pub struct Topup<'a>(&'a Client);
impl Topup<'_> {
    /// Stripe checkout — a top-up or a plan subscription. Open `pay_url` to pay.
    pub async fn card(&self, params: &CardCheckoutParams) -> Result<CardCheckout> {
        self.0.send(Req::post("/v1/topup/card", params)).await
    }

    /// Crypto top-up (pay-as-you-go only). Open `url` to pay.
    pub async fn crypto(&self, product_key: ProductKey, gb: u32) -> Result<CryptoCheckout> {
        self.0
            .send(Req::post(
                "/v1/topup/crypto",
                &serde_json::json!({ "kind": "topup", "productKey": product_key, "gb": gb }),
            ))
            .await
    }
}

/// Payment history.
pub struct Transactions<'a>(&'a Client);
impl Transactions<'_> {
    pub async fn list(&self, limit: Option<u32>) -> Result<Vec<Transaction>> {
        #[derive(serde::Deserialize)]
        struct R {
            transactions: Vec<Transaction>,
        }
        let mut req = Req::get("/v1/transactions");
        if let Some(l) = limit {
            req.query.push(("limit", l.to_string()));
        }
        Ok(self.0.send::<R>(req).await?.transactions)
    }
    pub async fn get(&self, id: &str) -> Result<Transaction> {
        self.0
            .send(Req::get(format!("/v1/transactions/{}", seg(id))))
            .await
    }

    /// Poll until the payment settles (`succeeded` or `failed`), giving up after `timeout`.
    pub async fn wait(
        &self,
        id: &str,
        interval: Duration,
        timeout: Duration,
    ) -> Result<Transaction> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let tx = self.get(id).await?;
            if tx.status != "pending" {
                return Ok(tx);
            }
            if std::time::Instant::now() + interval > deadline {
                return Err(Error::StillPending(id.into()));
            }
            tokio::time::sleep(interval).await;
        }
    }
}

/// Reseller API — needs a `bp_live_` key on a Business plan or higher.
pub struct Reseller<'a>(&'a Client);
impl<'a> Reseller<'a> {
    pub async fn me(&self) -> Result<ResellerSummary> {
        self.0.send(Req::get("/v1/me")).await
    }
    pub fn customers(&self) -> Customers<'a> {
        Customers(self.0)
    }
    pub fn pool(&self) -> Pool<'a> {
        Pool(self.0)
    }
}

pub struct Customers<'a>(&'a Client);
impl Customers<'_> {
    pub async fn list(&self, status: Option<CustomerStatus>) -> Result<Vec<Customer>> {
        #[derive(serde::Deserialize)]
        struct R {
            customers: Vec<Customer>,
        }
        let mut req = Req::get("/v1/customers");
        if let Some(s) = status {
            req.query.push((
                "status",
                match s {
                    CustomerStatus::Active => "active",
                    CustomerStatus::Suspended => "suspended",
                }
                .into(),
            ));
        }
        Ok(self.0.send::<R>(req).await?.customers)
    }
    /// Creates an isolated sub-account with no bandwidth — fund it with [`Customers::allocate`].
    pub async fn create(&self, label: &str) -> Result<Customer> {
        self.0
            .send(Req::post(
                "/v1/customers",
                &serde_json::json!({ "label": label }),
            ))
            .await
    }
    pub async fn get(&self, username: &str) -> Result<CustomerDetail> {
        self.0
            .send(Req::get(format!("/v1/customers/{}", seg(username))))
            .await
    }

    /// Rename and/or suspend / reactivate.
    pub async fn update(
        &self,
        username: &str,
        label: Option<&str>,
        status: Option<CustomerStatus>,
    ) -> Result<Customer> {
        let mut body = serde_json::Map::new();
        if let Some(l) = label {
            body.insert("label".into(), l.into());
        }
        if let Some(s) = status {
            body.insert("status".into(), serde_json::to_value(s).expect("status"));
        }
        let mut req = Req::post(format!("/v1/customers/{}", seg(username)), &body);
        req.method = Method::PATCH;
        self.0.send(req).await
    }
    pub async fn usage(&self, username: &str) -> Result<CustomerUsage> {
        self.0
            .send(Req::get(format!("/v1/customers/{}/usage", seg(username))))
            .await
    }

    /// Moves GB from your pool onto a customer. Set `idempotency_key` to make retries safe.
    pub async fn allocate(&self, username: &str, params: &AllocateParams) -> Result<Allocation> {
        let mut req = Req::post(
            format!("/v1/customers/{}/allocate", seg(username)),
            &serde_json::json!({ "productKey": params.product_key, "gb": params.gb }),
        );
        if let Some(k) = &params.idempotency_key {
            req.idempotency_key = Some(k.clone());
            req.retry_5xx = true;
        }
        self.0.send(req).await
    }

    pub async fn generate(
        &self,
        username: &str,
        plan_id: &str,
        params: &GenerateParams,
    ) -> Result<GeneratedProxies> {
        #[derive(serde::Deserialize)]
        struct R {
            lines: Vec<String>,
        }
        let r: R = self
            .0
            .send(Req::post(
                format!("/v1/customers/{}/generate", seg(username)),
                &generate_body(params, Some(("planId", plan_id))),
            ))
            .await?;
        GeneratedProxies::new(r.lines, params.protocol.unwrap_or_default())
    }
}

pub struct Pool<'a>(&'a Client);
impl Pool<'_> {
    /// Your unallocated bandwidth, per product.
    pub async fn get(&self) -> Result<Vec<PoolEntry>> {
        #[derive(serde::Deserialize)]
        struct R {
            pool: Vec<PoolEntry>,
        }
        Ok(self.0.send::<R>(Req::get("/v1/pool")).await?.pool)
    }
    pub async fn topup_card(&self, product_key: ProductKey, gb: u32) -> Result<CardCheckout> {
        self.0
            .send(Req::post(
                "/v1/pool/topup/card",
                &serde_json::json!({ "productKey": product_key, "gb": gb }),
            ))
            .await
    }
    pub async fn topup_crypto(&self, product_key: ProductKey, gb: u32) -> Result<CryptoCheckout> {
        self.0
            .send(Req::post(
                "/v1/pool/topup/crypto",
                &serde_json::json!({ "productKey": product_key, "gb": gb }),
            ))
            .await
    }
}
