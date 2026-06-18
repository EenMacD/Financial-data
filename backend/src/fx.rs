//! Ebury FX mock: mid-market rates, 2% FX markup, and quote computation.
//!
//! Rates are expressed as "units of currency per 1 USD". Cross rates are
//! derived through USD so any supported currency can convert to any other.

use serde::Serialize;

/// Platform revenue knobs.
pub const FX_MARKUP: f64 = 0.02; // 2% spread added on top of the mid-market rate
pub const PROCESSING_FEE: f64 = 0.01; // 1% processing fee on the converted amount

/// Currencies the platform supports, with their mid-market rate vs USD.
/// (Static table for the MVP — in production this would stream from Ebury.)
pub const RATES: &[(&str, f64)] = &[
    ("USD", 1.0),
    ("EUR", 0.92),
    ("GBP", 0.79),
    ("AUD", 1.52),
    ("CAD", 1.36),
    ("ZAR", 18.45), // South African Rand
    ("KES", 129.0), // Kenyan Shilling
    ("TZS", 2550.0), // Tanzanian Shilling
];

pub fn usd_rate(code: &str) -> Option<f64> {
    RATES
        .iter()
        .find(|(c, _)| c.eq_ignore_ascii_case(code))
        .map(|(_, r)| *r)
}

/// Mid-market cross rate: how many units of `to` for 1 unit of `from`.
pub fn mid_rate(from: &str, to: &str) -> Option<f64> {
    let from_r = usd_rate(from)?;
    let to_r = usd_rate(to)?;
    Some(to_r / from_r)
}

#[derive(Debug, Clone, Serialize)]
pub struct Quote {
    /// Currency the operator wants to be paid in (their wallet currency).
    pub settlement_currency: String,
    /// Amount the operator receives in the settlement currency.
    pub settlement_amount: f64,
    /// Currency the international guest is charged in.
    pub presentment_currency: String,
    /// True mid-market rate (settlement -> presentment).
    pub mid_rate: f64,
    /// Rate quoted to the guest, including the 2% markup.
    pub quoted_rate: f64,
    /// Converted amount in the presentment currency, before the processing fee.
    pub converted_amount: f64,
    /// 1% processing fee in the presentment currency.
    pub processing_fee: f64,
    /// What the guest is actually charged in the presentment currency.
    pub total_charged: f64,
    /// FX markup revenue, normalised to USD for reporting.
    pub fx_markup_revenue_usd: f64,
    /// Processing fee revenue, normalised to USD for reporting.
    pub processing_fee_revenue_usd: f64,
}

/// Build a quote for an operator wanting `settlement_amount` of
/// `settlement_currency`, charged to a guest in `presentment_currency`.
pub fn quote(
    settlement_currency: &str,
    settlement_amount: f64,
    presentment_currency: &str,
) -> Result<Quote, String> {
    let mid = mid_rate(settlement_currency, presentment_currency)
        .ok_or_else(|| "unsupported currency".to_string())?;
    let presentment_usd = usd_rate(presentment_currency).unwrap();

    let quoted_rate = mid * (1.0 + FX_MARKUP);
    let converted_amount = round2(settlement_amount * quoted_rate);
    let processing_fee = round2(converted_amount * PROCESSING_FEE);
    let total_charged = round2(converted_amount + processing_fee);

    // Markup revenue = the spread between quoted and mid, in presentment ccy.
    let markup_revenue_presentment = settlement_amount * mid * FX_MARKUP;
    let fx_markup_revenue_usd = round2(markup_revenue_presentment / presentment_usd);
    let processing_fee_revenue_usd = round2(processing_fee / presentment_usd);

    Ok(Quote {
        settlement_currency: settlement_currency.to_uppercase(),
        settlement_amount: round2(settlement_amount),
        presentment_currency: presentment_currency.to_uppercase(),
        mid_rate: round6(mid),
        quoted_rate: round6(quoted_rate),
        converted_amount,
        processing_fee,
        total_charged,
        fx_markup_revenue_usd,
        processing_fee_revenue_usd,
    })
}

/// Convert any supported currency amount to USD (for portfolio totals).
pub fn to_usd(currency: &str, amount: f64) -> f64 {
    match usd_rate(currency) {
        Some(rate) => round2(amount / rate),
        None => 0.0,
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn round6(v: f64) -> f64 {
    (v * 1_000_000.0).round() / 1_000_000.0
}
