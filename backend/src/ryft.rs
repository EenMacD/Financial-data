//! Ryft gateway integration (mock seam).
//!
//! Ryft is both the card gateway and the KYB/KYC onboarding rail for
//! sub-accounts (one per travel business). This module is the single seam:
//! today it returns deterministic mock data so the whole product runs with no
//! credentials. When `RYFT_API_KEY` is set the real branch (TODO) calls
//! api.ryftpay.com — the call sites never change. Mirrors `fx.rs`'s mock style.

use std::env;

use uuid::Uuid;

/// True once real Ryft credentials are configured.
fn is_live() -> bool {
    env::var("RYFT_API_KEY")
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
}

/// A Ryft payment session: the hosted pay page a guest uses to pay by card.
#[derive(Debug, Clone)]
pub struct PaymentSession {
    pub session_id: String,
    /// Needed by the Ryft frontend SDK in a live integration; unused by the mock.
    #[allow(dead_code)]
    pub client_secret: String,
    pub hosted_url: String,
}

/// Create a payment session for `amount` in `currency`. `trip_id` is forwarded
/// as Ryft metadata (`metadata.tripId`) so reconciliation can match the
/// operator's own trip reference.
pub fn create_payment_session(
    amount: f64,
    currency: &str,
    trip_id: Option<&str>,
    _customer_email: Option<&str>,
) -> PaymentSession {
    if is_live() {
        // TODO(real): POST https://api.ryftpay.com/v1/payment-sessions
        //   { amount: <minor units>, currency, metadata: { tripId },
        //     successUrl, ... } with `Authorization: Bearer $RYFT_API_KEY`,
        //   returning the live id + clientSecret + hosted payment URL.
        // Falls through to the mock below until wired.
    }
    let _ = (amount, trip_id);
    let session_id = format!("ps_{}", Uuid::new_v4().simple());
    let secret_suffix = Uuid::new_v4().simple().to_string();
    PaymentSession {
        client_secret: format!("{session_id}_secret_{}", &secret_suffix[..12]),
        hosted_url: format!(
            "https://pay.ryftpay.com/{session_id}?ccy={}",
            currency.to_uppercase()
        ),
        session_id,
    }
}

/// Result of kicking off Ryft sub-account onboarding (KYB/KYC).
#[derive(Debug, Clone)]
pub struct Onboarding {
    pub subaccount_id: String,
    pub onboarding_url: String,
    /// "pending" until KYB/KYC review completes.
    pub status: String,
}

/// Begin hosted KYB/KYC onboarding for a new sub-account (travel business).
pub fn create_subaccount_onboarding(business_name: &str) -> Onboarding {
    if is_live() {
        // TODO(real): POST https://api.ryftpay.com/v1/sub-accounts with the
        //   business + persons payload (or the Hosted onboarding variant),
        //   returning the sub-account id + a hosted onboarding redirect URL.
    }
    let _ = business_name;
    let subaccount_id = format!("sa_{}", Uuid::new_v4().simple());
    Onboarding {
        onboarding_url: format!("https://onboarding.ryftpay.com/{subaccount_id}"),
        subaccount_id,
        status: "pending".to_string(),
    }
}
