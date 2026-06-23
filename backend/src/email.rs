//! Transactional email via Resend (mock seam).
//!
//! Sign-in links are bearer credentials, so for self-service login they must be
//! delivered to the address owner's inbox (the inbox proves identity) rather than
//! returned to the API caller. This module is the single seam: with no
//! `RESEND_API_KEY` it sends nothing and reports `Ok(false)` so the caller falls
//! back to the dev `EXPOSE_MAGIC_LINK` / server-log path. Mirrors `ryft.rs`'s
//! mock style — the call sites never change.

use std::env;

const RESEND_ENDPOINT: &str = "https://api.resend.com/emails";

/// Sender identity for outbound mail. Override via `EMAIL_FROM`; the default is a
/// Resend sandbox sender that only delivers to your own Resend-account address
/// (verify a domain to reach external recipients).
fn from_address() -> String {
    env::var("EMAIL_FROM").unwrap_or_else(|_| "money·data <onboarding@resend.dev>".to_string())
}

/// Email a one-time sign-in link to `to`. Returns `Ok(true)` when a message was
/// actually dispatched, `Ok(false)` when email is not configured (dev), and `Err`
/// when a configured send failed (network / Resend rejection).
pub async fn send_magic_link(to: &str, url: &str) -> Result<bool, reqwest::Error> {
    let api_key = match env::var("RESEND_API_KEY") {
        Ok(k) if !k.trim().is_empty() => k,
        _ => return Ok(false),
    };
    let html = format!(
        "<p>Click to sign in to money·data:</p>\
         <p><a href=\"{url}\">Sign in</a></p>\
         <p>This link is single-use and expires in 15 minutes. \
         If you didn't request it, you can ignore this email.</p>"
    );
    let body = serde_json::json!({
        "from": from_address(),
        "to": [to],
        "subject": "Your money·data sign-in link",
        "html": html,
    });
    reqwest::Client::new()
        .post(RESEND_ENDPOINT)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?
        .error_for_status()?;
    Ok(true)
}
