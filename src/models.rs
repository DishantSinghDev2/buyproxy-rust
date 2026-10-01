//! Typed request and response models.
use serde::{Deserialize, Serialize};

use crate::proxy::Protocol;

/// Accepted by every top-up and allocation call — see [`crate::Client::products`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProductKey {
    Residential,
    Premium,
    Isp,
    Mobile,
    Ipv4,
    Ipv6,
    /// A product added after this SDK version.
    #[serde(other)]
    Other,
}

/// Plans you can buy by card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaidPlan {
    Pro,
    Business,
    Advanced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BillingInterval {
    Month,
    Year,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CustomerStatus {
    Active,
    Suspended,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    pub product_key: ProductKey,
    pub product_id: String,
    pub name: String,
    pub description: String,
    pub min_gb: u32,
    /// `None` if pricing is momentarily unavailable.
    pub price_per_gb_cents: Option<u32>,
    pub currency: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeBonus {
    pub threshold_gb: u32,
    pub rate: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanPrice {
    pub plan: PaidPlan,
    pub monthly_price_cents: u32,
    pub included_gb: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub products: Vec<Product>,
    pub volume_bonus: VolumeBonus,
    pub plans: Vec<PlanPrice>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub email: String,
    pub name: String,
    pub plan: String,
    pub plan_expires_at: Option<String>,
    /// Lifetime GB credited. Spendable per-product balances come from `balances()`.
    pub balance_gb: f64,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Balance {
    pub product_key: Option<ProductKey>,
    pub product_id: String,
    /// What to pass to `plans().generate`; `None` until the product has been funded once.
    pub plan_id: Option<String>,
    pub active_gb: f64,
    /// Paid for but still being added to the plan.
    pub pending_gb: f64,
    pub total_gb: f64,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Targeting {
    pub country: bool,
    pub state: bool,
    pub city: bool,
    pub isp: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateCapabilities {
    pub max_count: u32,
    /// `0` means no sticky sessions on this product.
    pub max_sticky_minutes: u32,
    pub protocols: Vec<Protocol>,
    pub targeting: Targeting,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: String,
    pub product_id: String,
    pub product_key: Option<ProductKey>,
    pub name: String,
    pub label: Option<String>,
    pub data_gb_remaining: f64,
    pub data_gb_total: f64,
    pub expires_at: Option<String>,
    pub created_at: String,
    pub generate: Option<GenerateCapabilities>,
}

/// Parameters for proxy generation. Unset fields use the API's defaults.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<Protocol>,
    /// 1–1000, default 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// ISO 3166-1 alpha-2, e.g. `"US"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isp: Option<String>,
    /// Keep the same IP this long; leave `None` for a fresh IP per request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sticky_minutes: Option<u32>,
}

impl GenerateParams {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn protocol(mut self, p: Protocol) -> Self {
        self.protocol = Some(p);
        self
    }
    pub fn count(mut self, n: u32) -> Self {
        self.count = Some(n);
        self
    }
    pub fn country(mut self, c: impl Into<String>) -> Self {
        self.country = Some(c.into());
        self
    }
    pub fn state(mut self, s: impl Into<String>) -> Self {
        self.state = Some(s.into());
        self
    }
    pub fn city(mut self, c: impl Into<String>) -> Self {
        self.city = Some(c.into());
        self
    }
    pub fn isp(mut self, i: impl Into<String>) -> Self {
        self.isp = Some(i.into());
        self
    }
    pub fn sticky_minutes(mut self, m: u32) -> Self {
        self.sticky_minutes = Some(m);
        self
    }
}

/// A card checkout request: a top-up or a plan subscription.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CardCheckoutParams {
    #[serde(rename_all = "camelCase")]
    Topup { product_key: ProductKey, gb: u32 },
    Plan {
        plan: PaidPlan,
        #[serde(skip_serializing_if = "Option::is_none")]
        interval: Option<BillingInterval>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardCheckout {
    /// Stripe Checkout, or the subscription settings page when switching an existing plan.
    pub pay_url: String,
    /// `None` when `pay_url` is the subscription portal rather than a new payment.
    pub transaction_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptoCheckout {
    pub url: String,
    pub transaction_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    pub id: String,
    pub kind: String,
    /// `pending`, `succeeded` or `failed`.
    pub status: String,
    pub payment_method: String,
    pub interval: Option<BillingInterval>,
    pub amount_cents: u64,
    pub currency: String,
    /// GB credited, including any volume bonus.
    pub gb: Option<f64>,
    pub product_key: Option<ProductKey>,
    pub plan: Option<String>,
    pub reseller_pool: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolEntry {
    pub product_key: ProductKey,
    pub product_id: String,
    pub label: String,
    pub gb: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResellerSummary {
    pub email: String,
    pub customer_count: u32,
    pub active_customer_count: u32,
    pub pool: Vec<PoolEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Customer {
    pub username: String,
    pub label: String,
    pub status: CustomerStatus,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingAllocation {
    pub product_id: String,
    pub product_key: Option<ProductKey>,
    pub gb: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerPlan {
    #[serde(flatten)]
    pub plan: Plan,
    pub remaining_gb: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CustomerDetail {
    #[serde(flatten)]
    pub customer: Customer,
    pub plans: Vec<CustomerPlan>,
    /// GB allocated but still being added to a plan.
    pub pending: Vec<PendingAllocation>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePlan {
    pub id: String,
    pub product_id: String,
    pub product_key: Option<ProductKey>,
    pub remaining_gb: f64,
    pub total_gb: f64,
    pub used_gb: f64,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CustomerUsage {
    pub plans: Vec<UsagePlan>,
    pub pending: Vec<PendingAllocation>,
}

/// Move GB from your pool onto a customer.
#[derive(Debug, Clone)]
pub struct AllocateParams {
    pub product_key: ProductKey,
    /// Whole GB, at least the product's minimum.
    pub gb: u32,
    /// Makes retries safe: repeating a key returns `already_applied` and moves nothing.
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Allocation {
    pub product_key: ProductKey,
    pub product_id: String,
    pub gb: u32,
    pub already_applied: bool,
    pub pool_remaining_gb: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignupResult {
    pub ok: bool,
    pub cooldown_seconds: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VerifiedUser {
    pub email: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResult {
    pub ok: bool,
    pub is_new: bool,
    pub user: VerifiedUser,
}
