//! Domain models for the payment orchestration platform.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct Operator {
    pub id: String,
    pub name: String,
    pub region: String,
    pub settlement_currency: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaymentStatus {
    /// Link generated, guest has not paid yet.
    Pending,
    /// Guest paid via Ecommpay; funds sit in the wallet's pending balance.
    Paid,
    /// Swept into the wallet's available balance by the daily batch.
    Settled,
    /// Refunded via card reversal or direct debit pull.
    Refunded,
}

impl PaymentStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            PaymentStatus::Pending => "pending",
            PaymentStatus::Paid => "paid",
            PaymentStatus::Settled => "settled",
            PaymentStatus::Refunded => "refunded",
        }
    }

    pub fn parse(s: &str) -> PaymentStatus {
        match s {
            "paid" => PaymentStatus::Paid,
            "settled" => PaymentStatus::Settled,
            "refunded" => PaymentStatus::Refunded,
            _ => PaymentStatus::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PaymentLink {
    pub id: String,
    pub tenant_id: String,
    pub reference: String,
    /// Operator's own external reference for the trip being paid for.
    pub trip_id: Option<String>,
    pub guest_name: String,
    pub description: String,
    pub settlement_currency: String,
    pub settlement_amount: f64,
    pub presentment_currency: String,
    pub mid_rate: f64,
    pub quoted_rate: f64,
    pub converted_amount: f64,
    pub processing_fee: f64,
    pub total_charged: f64,
    pub fx_markup_revenue_usd: f64,
    pub processing_fee_revenue_usd: f64,
    pub status: PaymentStatus,
    pub url: String,
    /// Ryft payment-session id backing the hosted pay page (mock seam).
    pub ryft_session_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub paid_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RefundMethod {
    /// Reverse the original card charge through Ecommpay.
    CardReversal,
    /// Pull funds back from the operator wallet via an Ebury direct debit.
    DirectDebitPull,
}

impl RefundMethod {
    pub fn as_db(self) -> &'static str {
        match self {
            RefundMethod::CardReversal => "card_reversal",
            RefundMethod::DirectDebitPull => "direct_debit_pull",
        }
    }

    pub fn parse(s: &str) -> RefundMethod {
        match s {
            "direct_debit_pull" => RefundMethod::DirectDebitPull,
            _ => RefundMethod::CardReversal,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Refund {
    pub id: String,
    pub payment_id: String,
    pub payment_reference: String,
    pub method: RefundMethod,
    pub amount: f64,
    pub currency: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettlementLine {
    pub currency: String,
    pub amount: f64,
    pub payments: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettlementBatch {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub lines: Vec<SettlementLine>,
    pub total_usd: f64,
}

// ---------------------------------------------------------------------------
// Tenancy & auth
// ---------------------------------------------------------------------------

/// A travel business onboarded onto the platform (Ryft sub-account).
#[derive(Debug, Clone, Serialize)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub region: String,
    pub settlement_currency: String,
    pub registration_number: Option<String>,
    pub country: Option<String>,
    pub contact_name: Option<String>,
    pub contact_email: Option<String>,
    pub ryft_subaccount_id: Option<String>,
    /// "pending" | "in_review" | "verified" | "rejected"
    pub onboarding_status: String,
    pub onboarding_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A person who can sign in. `role` is "admin" (platform, spans all tenants) or
/// "tenant" (pinned to `tenant_id`).
#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: String,
    pub email: String,
    pub name: String,
    pub role: String,
    pub tenant_id: Option<String>,
    pub created_at: DateTime<Utc>,
}
