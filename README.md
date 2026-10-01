# BuyProxy Rust SDK — Residential Proxy API Client Crate

[![crates.io](https://img.shields.io/crates/v/buyproxy?logo=rust&logoColor=white)](https://crates.io/crates/buyproxy)
[![docs.rs](https://img.shields.io/docsrs/buyproxy)](https://docs.rs/buyproxy)
[![CI](https://github.com/DishantSinghDev2/buyproxy-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/DishantSinghDev2/buyproxy-rust/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-green)](LICENSE)

The official **Rust crate** for [BuyProxy](https://www.buyproxy.org): generate **residential, mobile, ISP and datacenter proxies**, check bandwidth, top up by card or crypto, and manage **reseller** customers. Async on `tokio`/`reqwest` with rustls, builder-style params and a typed `Error`.

**Keywords:** residential proxies · rotating proxies · mobile proxies · ISP proxies · datacenter proxies · SOCKS5 · proxy API · web scraping · proxy reseller

## Features

- Async, `Clone`-cheap `Client`, safe to share between tasks
- Builder-style `GenerateParams` and a single typed `Error` with `is_*()` helpers
- `Proxy::to_reqwest()` plugs straight into `reqwest`
- Automatic retries with back-off for rate limits and transient errors

## Installation

```bash
cargo add buyproxy tokio --features tokio/full
```

Rust 1.75+. TLS via rustls — no OpenSSL needed.

## Quick start

```rust
use buyproxy::{Client, GenerateParams};

#[tokio::main]
async fn main() -> buyproxy::Result<()> {
    let client = Client::new(); // reads BUYPROXY_API_KEY — or Client::builder().api_key("bp_key_…").build()

    let plans = client.plans().list().await?;
    let out = client
        .plans()
        .generate(&plans[0].id, &GenerateParams::new().country("US").city("Austin").count(5))
        .await?;

    println!("{}", out.proxies[0].url()); // http://user-country-us-city-austin:pass@gw.buyproxy.org:6969
    Ok(())
}
```

Add `.sticky_minutes(10)` to keep the same IP for ten minutes, or `.protocol(Protocol::Socks5)` for SOCKS5.

## Using the proxies with your stack

### reqwest

```rust
let http = reqwest::Client::builder().proxy(out.proxies[0].to_reqwest()?).build()?;
println!("{}", http.get("https://api.ipify.org").send().await?.text().await?);
```

### curl & shell

```rust
println!("{}", out.proxies[0].curl());   // ready-to-run curl command
println!("{:?}", out.proxies[0].env());  // HTTP_PROXY, HTTPS_PROXY, ALL_PROXY, …
```

## Buy bandwidth (card & crypto)

```rust
use buyproxy::{BillingInterval, CardCheckoutParams, PaidPlan, ProductKey};

// Buy 10 GB of Core Residential by card — open pay_url in a browser
let checkout = client.topup().card(&CardCheckoutParams::Topup { product_key: ProductKey::Residential, gb: 10 }).await?;
println!("{}", checkout.pay_url);

// Or subscribe to a plan (monthly or yearly)
client.topup().card(&CardCheckoutParams::Plan { plan: PaidPlan::Pro, interval: Some(BillingInterval::Year) }).await?;

// Crypto (pay-as-you-go top-ups only)
client.topup().crypto(ProductKey::Residential, 50).await?;

// Wait for the payment to settle — GB appears on client.balances() right after
let tx = client.transactions().wait(&checkout.transaction_id.unwrap(), Duration::from_secs(3), Duration::from_secs(600)).await?;
```

Card payments run through Stripe Checkout; crypto is available for pay-as-you-go top-ups.

## Reseller API

Build a proxy product on top of BuyProxy: buy bandwidth in bulk into a **reseller pool**, create isolated **customers**, and allocate GB to them. Needs a `bp_live_` key on a Business plan or higher.

```rust
let reseller = Client::builder().api_key("bp_live_…").build();

reseller.reseller().pool().topup_card(ProductKey::Residential, 500).await?;       // buy into your pool
let customer = reseller.reseller().customers().create("Acme Inc").await?;         // isolated sub-account

reseller.reseller().customers().allocate(&customer.username, &AllocateParams {
    product_key: ProductKey::Residential, gb: 20, idempotency_key: Some("order-1042".into()), // safe to retry
}).await?;

let detail = reseller.reseller().customers().get(&customer.username).await?;
let out = reseller.reseller().customers()
    .generate(&customer.username, &detail.plans[0].plan.id, &GenerateParams::new().country("DE")).await?;

reseller.reseller().customers().update(&customer.username, None, Some(CustomerStatus::Suspended)).await?;
```

## Error handling

```rust
match client.plans().generate(&plan_id, &GenerateParams::new()).await {
    Ok(out) => println!("{} proxies", out.proxies.len()),
    Err(e) if e.is_insufficient_bandwidth() => println!("Out of GB — top up first"),
    Err(e) if e.is_rate_limited() => println!("Slow down: {e}"),
    Err(e) if e.is_authentication() => println!("Check your API key"),
    Err(e) => return Err(e),
}
```

## Configuration

```rust
let client = Client::builder()
    .api_key("bp_key_…")                         // default: $BUYPROXY_API_KEY
    .base_url("https://www.buyproxy.org")
    .timeout(Duration::from_secs(30))
    .max_retries(2)                               // 429s always; 5xx only for safe requests
    .http_client(my_reqwest_client)               // bring your own reqwest::Client
    .build();
```

## API overview

| Capability | Call |
|---|---|
| Catalog & prices | `client.products()` |
| Account | `client.account()` |
| Spendable GB per product | `client.balances()` |
| Plans | `client.plans().list()` |
| Generate proxies | `client.plans().generate(id, &params)` |
| Card checkout | `client.topup().card(&params)` |
| Crypto checkout | `client.topup().crypto(product, gb)` |
| Payments | `client.transactions().list/get/wait` |
| Create an account | `client.signup()` · `.verify_otp()` · `.resend_otp()` |
| Reseller summary | `client.reseller().me()` |
| Customers | `client.reseller().customers().list/create/get/update/usage/allocate/generate` |
| Reseller pool | `client.reseller().pool().get/topup_card/topup_crypto` |

Full endpoint reference: <https://www.buyproxy.org/docs/api-reference/introduction> · OpenAPI spec: <https://www.buyproxy.org/openapi.yaml>

## FAQ

**How do I get an API key?**
Create a free account at [buyproxy.org](https://www.buyproxy.org/signup), then open **Dashboard → API keys**. Personal keys start with `bp_key_`; reseller keys (Business plan and above) start with `bp_live_`. You can create the *account* from code (`signup` + `verify otp`), but keys are issued from the dashboard.

**What's the difference between a personal and a reseller key?**
A personal key (`bp_key_…`) manages your own bandwidth: balances, plans, generating proxies, top-ups and payments. A reseller key (`bp_live_…`) manages *your customers*: create isolated sub-accounts, buy bandwidth into a pool, allocate it, suspend or reactivate, and generate proxies on their behalf.

**Which proxy types are available?**
Core Residential, Premium Residential, ISP Rotating, Mobile, Datacenter IPv4 and IPv6 — call the catalog endpoint for the live list and per-GB prices (from **$1.99/GB**).

**Do generated proxies expire, and when is bandwidth used?**
Generating a proxy is free and instant; bandwidth is only consumed when traffic flows through it. Lines are on-demand — generate fresh ones rather than storing them.

**What are the rate limits?**
60 requests per minute per API key. The SDK honours `Retry-After` and retries rate-limited requests automatically.

**Is it free to use the SDK?**
Yes — the SDK is MIT-licensed. You pay only for the proxy bandwidth you buy.

## Other BuyProxy SDKs

[Node.js](https://github.com/DishantSinghDev2/buyproxy-node) · [Python](https://github.com/DishantSinghDev2/buyproxy-python) · [Go](https://github.com/DishantSinghDev2/buyproxy-go) · [PHP](https://github.com/DishantSinghDev2/buyproxy-php) · [Ruby](https://github.com/DishantSinghDev2/buyproxy-ruby) · [Java](https://github.com/DishantSinghDev2/buyproxy-java) · [.NET](https://github.com/DishantSinghDev2/buyproxy-dotnet) · [Rust](https://github.com/DishantSinghDev2/buyproxy-rust) · [CLI](https://github.com/DishantSinghDev2/buyproxy-cli)

All SDKs are checked against the same request log, so behaviour is identical across languages — see [buyproxy-sdks](https://github.com/DishantSinghDev2/buyproxy-sdks).

## Development

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

Issues and pull requests are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md). Security reports: [SECURITY.md](SECURITY.md).

## Links

- Website: <https://www.buyproxy.org>
- Documentation: <https://www.buyproxy.org/docs/sdks>
- Pricing: <https://www.buyproxy.org/pricing>
- Support: <support@buyproxy.org>

## License

[MIT](LICENSE) © BuyProxy
