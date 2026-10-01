use std::time::Duration;

use buyproxy::{
    AllocateParams, CardCheckoutParams, Client, Error, GenerateParams, ProductKey, Protocol, Proxy,
};
use wiremock::matchers::{body_json, header, header_exists, method, path};
use wiremock::{Mock, MockServer, Respond, ResponseTemplate};

fn client(server: &MockServer) -> Client {
    Client::builder()
        .api_key("bp_key_x")
        .base_url(server.uri())
        .build()
}

#[test]
fn parses_proxy_lines_keeping_at_and_colon_in_password() {
    let p = Proxy::parse(
        "user-country-us:p@ss:w0rd@gw.buyproxy.org:6969",
        Protocol::Socks5,
    )
    .unwrap();
    assert_eq!(
        (
            p.host.as_str(),
            p.port,
            p.username.as_str(),
            p.password.as_str()
        ),
        ("gw.buyproxy.org", 6969, "user-country-us", "p@ss:w0rd")
    );
    assert_eq!(
        p.url(),
        "socks5://user-country-us:p%40ss%3Aw0rd@gw.buyproxy.org:6969"
    );
    assert_eq!(
        p.to_string(),
        "user-country-us:p@ss:w0rd@gw.buyproxy.org:6969"
    );
    for bad in [
        "",
        "nope",
        "u:p@host",
        "u:p@host:99999",
        ":p@host:80",
        "u@host:80",
    ] {
        assert!(
            Proxy::parse(bad, Protocol::Http).is_err(),
            "{bad:?} should be rejected"
        );
    }
}

#[tokio::test]
async fn generate_sends_key_body_and_parses() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/plans/plan%201/generate"))
        .and(header("x-api-key", "bp_key_x"))
        .and(header_exists("user-agent"))
        .and(body_json(
            serde_json::json!({ "protocol": "socks5", "count": 1 }),
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "lines": ["u:p@gw.buyproxy.org:6969"] })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let out = client(&server)
        .plans()
        .generate(
            "plan 1",
            &GenerateParams::new().protocol(Protocol::Socks5).count(1),
        )
        .await
        .unwrap();
    assert_eq!(out.proxies[0].protocol, Protocol::Socks5);
}

#[tokio::test]
async fn maps_statuses_to_error_helpers() {
    for (status, check) in [
        (401u16, Error::is_authentication as fn(&Error) -> bool),
        (402, Error::is_insufficient_bandwidth),
        (404, Error::is_not_found),
        (403, Error::is_forbidden),
    ] {
        let server = MockServer::start().await;
        Mock::given(path("/v1/account"))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(serde_json::json!({ "error": "nope" })),
            )
            .mount(&server)
            .await;
        let err = client(&server).account().await.unwrap_err();
        assert!(check(&err), "status {status}: {err}");
        assert!(err.to_string().contains("nope"));
    }
}

struct FailOnce(std::sync::atomic::AtomicBool);
impl Respond for FailOnce {
    fn respond(&self, _: &wiremock::Request) -> ResponseTemplate {
        if self.0.swap(false, std::sync::atomic::Ordering::SeqCst) {
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_json(serde_json::json!({ "error": "slow" }))
        } else {
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "email": "a@b.co", "name": "A", "plan": "pro", "planExpiresAt": null, "balanceGb": 4.2, "createdAt": "t" }))
        }
    }
}

#[tokio::test]
async fn retries_429_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/account"))
        .respond_with(FailOnce(true.into()))
        .expect(2)
        .mount(&server)
        .await;
    assert_eq!(client(&server).account().await.unwrap().plan, "pro");
}

#[tokio::test]
async fn post_not_retried_on_503_unless_idempotent() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/customers/c/allocate"))
        .respond_with(
            ResponseTemplate::new(503).set_body_json(serde_json::json!({ "error": "down" })),
        )
        .expect(1)
        .mount(&server)
        .await;
    let err = client(&server)
        .reseller()
        .customers()
        .allocate(
            "c",
            &AllocateParams {
                product_key: ProductKey::Premium,
                gb: 1,
                idempotency_key: None,
            },
        )
        .await
        .unwrap_err();
    assert!(err.is_server_error());
}

#[tokio::test]
async fn missing_key_never_hits_the_network_but_catalog_is_public() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/products")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "products": [], "volumeBonus": { "thresholdGb": 100, "rate": 0.1 }, "plans": [] }))).expect(1).mount(&server).await;
    let c = Client::builder().base_url(server.uri()).build();
    let c = if std::env::var("BUYPROXY_API_KEY").is_ok() {
        return;
    } else {
        c
    };
    assert!(matches!(c.account().await, Err(Error::MissingApiKey)));
    assert_eq!(c.products().await.unwrap().volume_bonus.threshold_gb, 100);
}

#[tokio::test]
async fn card_checkout_params_serialise_like_the_api_expects() {
    let topup = serde_json::to_value(CardCheckoutParams::Topup {
        product_key: ProductKey::Residential,
        gb: 10,
    })
    .unwrap();
    assert_eq!(
        topup,
        serde_json::json!({ "kind": "topup", "productKey": "residential", "gb": 10 })
    );
    let plan = serde_json::to_value(CardCheckoutParams::Plan {
        plan: buyproxy::PaidPlan::Pro,
        interval: Some(buyproxy::BillingInterval::Year),
    })
    .unwrap();
    assert_eq!(
        plan,
        serde_json::json!({ "kind": "plan", "plan": "pro", "interval": "year" })
    );
}

#[tokio::test]
async fn wait_polls_until_settled() {
    let server = MockServer::start().await;
    let tx = |status: &str| serde_json::json!({ "id": "st_1", "kind": "topup", "status": status, "paymentMethod": "card", "interval": null, "amountCents": 1, "currency": "usd", "gb": 1, "productKey": null, "plan": null, "resellerPool": false, "createdAt": "t" });
    Mock::given(path("/v1/transactions/st_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(tx("succeeded")))
        .mount(&server)
        .await;
    let out = client(&server)
        .transactions()
        .wait("st_1", Duration::from_millis(1), Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(out.status, "succeeded");
}
