//! HTTP handlers for the orchestration API (Postgres-backed, tenant-scoped).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::{self, AuthCtx};
use crate::db;
use crate::fx;
use crate::models::*;

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ErrorBody {
    pub error: String,
}

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(ErrorBody { error: self.1 })).into_response()
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

fn internal<E: std::fmt::Display>(e: E) -> ApiError {
    // Log the real cause server-side; never leak DB/internal detail to clients.
    tracing::error!("internal error: {e}");
    ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal server error".to_string(),
    )
}
fn bad(m: &str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, m.to_string())
}
fn conflict(m: &str) -> ApiError {
    ApiError(StatusCode::CONFLICT, m.to_string())
}
fn not_found(m: &str) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, m.to_string())
}
fn forbidden(m: &str) -> ApiError {
    ApiError(StatusCode::FORBIDDEN, m.to_string())
}

// ---------------------------------------------------------------------------
// Row types (DB <-> API model mapping)
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct LinkRow {
    id: String,
    tenant_id: String,
    reference: String,
    trip_id: Option<String>,
    guest_name: String,
    description: String,
    settlement_currency: String,
    settlement_amount: f64,
    presentment_currency: String,
    mid_rate: f64,
    quoted_rate: f64,
    converted_amount: f64,
    processing_fee: f64,
    total_charged: f64,
    fx_markup_revenue_usd: f64,
    processing_fee_revenue_usd: f64,
    status: String,
    url: String,
    ryft_session_id: Option<String>,
    created_at: DateTime<Utc>,
    paid_at: Option<DateTime<Utc>>,
}

impl From<LinkRow> for PaymentLink {
    fn from(r: LinkRow) -> Self {
        PaymentLink {
            id: r.id,
            tenant_id: r.tenant_id,
            reference: r.reference,
            trip_id: r.trip_id,
            guest_name: r.guest_name,
            description: r.description,
            settlement_currency: r.settlement_currency,
            settlement_amount: r.settlement_amount,
            presentment_currency: r.presentment_currency,
            mid_rate: r.mid_rate,
            quoted_rate: r.quoted_rate,
            converted_amount: r.converted_amount,
            processing_fee: r.processing_fee,
            total_charged: r.total_charged,
            fx_markup_revenue_usd: r.fx_markup_revenue_usd,
            processing_fee_revenue_usd: r.processing_fee_revenue_usd,
            status: PaymentStatus::parse(&r.status),
            url: r.url,
            ryft_session_id: r.ryft_session_id,
            created_at: r.created_at,
            paid_at: r.paid_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct RefundRow {
    id: String,
    payment_id: String,
    payment_reference: String,
    method: String,
    amount: f64,
    currency: String,
    status: String,
    created_at: DateTime<Utc>,
}

impl From<RefundRow> for Refund {
    fn from(r: RefundRow) -> Self {
        Refund {
            id: r.id,
            payment_id: r.payment_id,
            payment_reference: r.payment_reference,
            method: RefundMethod::parse(&r.method),
            amount: r.amount,
            currency: r.currency,
            status: r.status,
            created_at: r.created_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Health & reference data
// ---------------------------------------------------------------------------

pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "service": "money-data" }))
}

#[derive(Serialize)]
pub struct RateEntry {
    pub currency: String,
    pub usd_rate: f64,
}

pub async fn list_rates() -> Json<Vec<RateEntry>> {
    let rates = fx::RATES
        .iter()
        .map(|(c, r)| RateEntry { currency: c.to_string(), usd_rate: *r })
        .collect();
    Json(rates)
}

// ---------------------------------------------------------------------------
// Auth: magic-link sign-in, invites, current user
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct MagicLinkReq {
    pub email: String,
}

#[derive(Serialize)]
pub struct MagicLinkResp {
    pub sent: bool,
    /// The copyable sign-in link (mock email). `None` if no account matches.
    pub magic_link: Option<String>,
}

pub async fn request_magic_link(
    State(pool): State<PgPool>,
    Json(req): Json<MagicLinkReq>,
) -> ApiResult<MagicLinkResp> {
    let email = req.email.trim().to_lowercase();
    if email.is_empty() {
        return Err(bad("email is required"));
    }
    // Only mint a link for a known account; stay silent otherwise (no enumeration).
    let resp = if db::find_user_by_email(&pool, &email)
        .await
        .map_err(internal)?
        .is_some()
    {
        let token = auth::random_token();
        let expires = Utc::now() + Duration::minutes(15);
        db::create_auth_token(&pool, &token, "magic", &email, None, None, None, expires)
            .await
            .map_err(internal)?;
        let link = format!("#/auth/verify?token={token}");
        // SECURITY: the link is a bearer credential. Always record it server-side
        // (retrievable from logs by the operator), but only return it in the HTTP
        // response when EXPOSE_MAGIC_LINK is enabled — otherwise an unauthenticated
        // caller could request a link for any known email and sign in as them.
        tracing::info!("magic-link sign-in for {email}: {link}");
        MagicLinkResp {
            sent: true,
            magic_link: if expose_magic_link() { Some(link) } else { None },
        }
    } else {
        MagicLinkResp { sent: true, magic_link: None }
    };
    Ok(Json(resp))
}

#[derive(Deserialize)]
pub struct VerifyReq {
    pub token: String,
}

#[derive(Serialize)]
pub struct SessionResp {
    pub token: String,
    pub user: User,
}

pub async fn verify_magic_link(
    State(pool): State<PgPool>,
    Json(req): Json<VerifyReq>,
) -> ApiResult<SessionResp> {
    let consumed = db::consume_token(&pool, &req.token, "magic")
        .await
        .map_err(internal)?
        .ok_or_else(|| bad("this sign-in link is invalid or has expired"))?;
    let user = db::find_user_by_email(&pool, &consumed.email)
        .await
        .map_err(internal)?
        .ok_or_else(|| bad("account not found"))?;
    let token = auth::issue_jwt(&user).map_err(internal)?;
    Ok(Json(SessionResp { token, user }))
}

#[derive(Deserialize)]
pub struct InviteReq {
    pub email: String,
    pub role: String,
    pub tenant_id: Option<String>,
}

#[derive(Serialize)]
pub struct InviteResp {
    pub accept_link: String,
    pub email: String,
}

/// Create an invite. Admins can invite admins or tenant users (to any tenant);
/// tenant users may only invite teammates into their own tenant.
pub async fn create_invite(
    State(pool): State<PgPool>,
    auth: AuthCtx,
    Json(req): Json<InviteReq>,
) -> ApiResult<InviteResp> {
    let email = req.email.trim().to_lowercase();
    if email.is_empty() {
        return Err(bad("email is required"));
    }
    let want_role = if req.role.eq_ignore_ascii_case("admin") {
        "admin"
    } else {
        "tenant"
    };
    if want_role == "admin" && !auth.is_admin() {
        return Err(forbidden("only admins can invite admins"));
    }
    let tenant_id: Option<String> = if want_role == "tenant" {
        Some(match (auth.is_admin(), req.tenant_id.clone()) {
            (true, Some(t)) => t,
            _ => db::resolve_tenant(&pool, &auth).await.map_err(internal)?,
        })
    } else {
        None
    };
    let token = auth::random_token();
    let expires = Utc::now() + Duration::days(7);
    db::create_auth_token(
        &pool,
        &token,
        "invite",
        &email,
        None,
        Some(want_role),
        tenant_id.as_deref(),
        expires,
    )
    .await
    .map_err(internal)?;
    Ok(Json(InviteResp {
        accept_link: format!("#/accept-invite?token={token}"),
        email,
    }))
}

#[derive(Deserialize)]
pub struct AcceptInviteReq {
    pub token: String,
    pub name: String,
}

pub async fn accept_invite(
    State(pool): State<PgPool>,
    Json(req): Json<AcceptInviteReq>,
) -> ApiResult<SessionResp> {
    let consumed = db::consume_token(&pool, &req.token, "invite")
        .await
        .map_err(internal)?
        .ok_or_else(|| bad("this invite is invalid or has expired"))?;
    let name = if req.name.trim().is_empty() {
        consumed.email.clone()
    } else {
        req.name.trim().to_string()
    };
    let role = consumed.role.as_deref().unwrap_or("tenant");
    let user = db::upsert_user(&pool, &consumed.email, &name, role, consumed.tenant_id.as_deref())
        .await
        .map_err(internal)?;
    let token = auth::issue_jwt(&user).map_err(internal)?;
    Ok(Json(SessionResp { token, user }))
}

#[derive(Serialize)]
pub struct MeResp {
    pub user: User,
    /// Tenants the user may act on (all of them for admins; their own otherwise).
    pub tenants: Vec<Tenant>,
    pub active_tenant_id: String,
}

pub async fn me(State(pool): State<PgPool>, auth: AuthCtx) -> ApiResult<MeResp> {
    let user = db::get_user(&pool, &auth.user_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found("user not found"))?;
    let tenants = if auth.is_admin() {
        db::list_tenants(&pool).await.map_err(internal)?
    } else {
        match &user.tenant_id {
            Some(t) => db::get_tenant(&pool, t)
                .await
                .map_err(internal)?
                .into_iter()
                .collect(),
            None => vec![],
        }
    };
    let active_tenant_id = db::resolve_tenant(&pool, &auth).await.map_err(internal)?;
    Ok(Json(MeResp { user, tenants, active_tenant_id }))
}

// ---------------------------------------------------------------------------
// Tenants (admin portal) & self-serve merchant sign-up (public)
// ---------------------------------------------------------------------------

pub async fn list_tenants(State(pool): State<PgPool>, auth: AuthCtx) -> ApiResult<Vec<Tenant>> {
    if !auth.is_admin() {
        return Err(forbidden("admin only"));
    }
    Ok(Json(db::list_tenants(&pool).await.map_err(internal)?))
}

#[derive(Deserialize)]
pub struct CreateTenantReq {
    pub name: String,
    pub region: Option<String>,
    pub settlement_currency: Option<String>,
    pub registration_number: Option<String>,
    pub country: Option<String>,
    pub contact_name: Option<String>,
    pub contact_email: Option<String>,
}

pub async fn create_tenant(
    State(pool): State<PgPool>,
    auth: AuthCtx,
    Json(req): Json<CreateTenantReq>,
) -> ApiResult<Tenant> {
    if !auth.is_admin() {
        return Err(forbidden("admin only"));
    }
    if req.name.trim().is_empty() {
        return Err(bad("business name is required"));
    }
    let tenant = db::create_tenant_with_setup(
        &pool,
        req.name.trim(),
        req.region.as_deref().unwrap_or(""),
        &req.settlement_currency.unwrap_or_else(|| "USD".to_string()),
        req.registration_number,
        req.country,
        req.contact_name,
        req.contact_email,
    )
    .await
    .map_err(internal)?;
    Ok(Json(tenant))
}

#[derive(Deserialize)]
pub struct SignupReq {
    pub business_name: String,
    pub registration_number: Option<String>,
    pub country: Option<String>,
    pub contact_name: Option<String>,
    pub contact_email: Option<String>,
    pub billing_currency: Option<String>,
}

#[derive(Serialize)]
pub struct SignupResp {
    pub tenant: Tenant,
    pub onboarding_url: Option<String>,
}

/// Public KYB/KYC sign-up: provisions a `pending` tenant + Ryft onboarding link.
pub async fn signup_merchant(
    State(pool): State<PgPool>,
    Json(req): Json<SignupReq>,
) -> ApiResult<SignupResp> {
    if req.business_name.trim().is_empty() {
        return Err(bad("business name is required"));
    }
    let region = req.country.clone().unwrap_or_default();
    let tenant = db::create_tenant_with_setup(
        &pool,
        req.business_name.trim(),
        &region,
        &req.billing_currency.unwrap_or_else(|| "USD".to_string()),
        req.registration_number,
        req.country,
        req.contact_name,
        req.contact_email,
    )
    .await
    .map_err(internal)?;
    let onboarding_url = tenant.onboarding_url.clone();
    Ok(Json(SignupResp { tenant, onboarding_url }))
}

// ---------------------------------------------------------------------------
// Tenant team users (Settings)
// ---------------------------------------------------------------------------

pub async fn list_users(State(pool): State<PgPool>, auth: AuthCtx) -> ApiResult<Vec<User>> {
    let tenant = db::resolve_tenant(&pool, &auth).await.map_err(internal)?;
    Ok(Json(
        db::list_tenant_users(&pool, &tenant).await.map_err(internal)?,
    ))
}

// ---------------------------------------------------------------------------
// Wallets
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct WalletView {
    pub currency: String,
    pub available: f64,
    pub pending: f64,
    pub available_usd: f64,
    pub pending_usd: f64,
}

#[derive(Serialize)]
pub struct WalletsResponse {
    pub wallets: Vec<WalletView>,
    pub total_available_usd: f64,
    pub total_pending_usd: f64,
}

pub async fn list_wallets(
    State(pool): State<PgPool>,
    auth: AuthCtx,
) -> ApiResult<WalletsResponse> {
    let tenant = db::resolve_tenant(&pool, &auth).await.map_err(internal)?;
    let rows = sqlx::query_as::<_, (String, f64, f64)>(
        "SELECT currency, available, pending FROM wallets WHERE tenant_id = $1 ORDER BY position, currency",
    )
    .bind(&tenant)
    .fetch_all(&pool)
    .await
    .map_err(internal)?;

    let mut total_available_usd = 0.0;
    let mut total_pending_usd = 0.0;
    let wallets = rows
        .into_iter()
        .map(|(currency, available, pending)| {
            let available_usd = fx::to_usd(&currency, available);
            let pending_usd = fx::to_usd(&currency, pending);
            total_available_usd += available_usd;
            total_pending_usd += pending_usd;
            WalletView { currency, available, pending, available_usd, pending_usd }
        })
        .collect();

    Ok(Json(WalletsResponse {
        wallets,
        total_available_usd: round2(total_available_usd),
        total_pending_usd: round2(total_pending_usd),
    }))
}

// ---------------------------------------------------------------------------
// Quotes
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct QuoteReq {
    pub settlement_currency: String,
    pub settlement_amount: f64,
    pub presentment_currency: String,
}

pub async fn create_quote(_auth: AuthCtx, Json(req): Json<QuoteReq>) -> ApiResult<fx::Quote> {
    if req.settlement_amount <= 0.0 {
        return Err(bad("amount must be positive"));
    }
    fx::quote(&req.settlement_currency, req.settlement_amount, &req.presentment_currency)
        .map(Json)
        .map_err(|e| bad(&e))
}

// ---------------------------------------------------------------------------
// Payment links / ledger
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateLinkReq {
    pub guest_name: String,
    pub description: String,
    pub settlement_currency: String,
    pub settlement_amount: f64,
    pub presentment_currency: String,
    pub trip_id: Option<String>,
}

pub async fn create_link(
    State(pool): State<PgPool>,
    auth: AuthCtx,
    Json(req): Json<CreateLinkReq>,
) -> ApiResult<PaymentLink> {
    if req.settlement_amount <= 0.0 {
        return Err(bad("amount must be positive"));
    }
    if req.guest_name.trim().is_empty() {
        return Err(bad("guest name is required"));
    }
    let tenant = db::resolve_tenant(&pool, &auth).await.map_err(internal)?;
    let link = db::build_link(
        &tenant,
        req.guest_name,
        req.description,
        &req.settlement_currency,
        req.settlement_amount,
        &req.presentment_currency,
        req.trip_id.filter(|t| !t.trim().is_empty()),
    )
    .map_err(|e| bad(&e))?;
    db::insert_link(&pool, &link).await.map_err(internal)?;
    Ok(Json(link))
}

pub async fn list_links(State(pool): State<PgPool>, auth: AuthCtx) -> ApiResult<Vec<PaymentLink>> {
    let tenant = db::resolve_tenant(&pool, &auth).await.map_err(internal)?;
    let rows = sqlx::query_as::<_, LinkRow>(
        "SELECT * FROM payment_links WHERE tenant_id = $1 ORDER BY created_at DESC",
    )
    .bind(&tenant)
    .fetch_all(&pool)
    .await
    .map_err(internal)?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

/// Simulate the guest completing payment through Ryft.
pub async fn pay_link(
    State(pool): State<PgPool>,
    auth: AuthCtx,
    Path(id): Path<String>,
) -> ApiResult<PaymentLink> {
    let tenant = db::resolve_tenant(&pool, &auth).await.map_err(internal)?;
    let mut tx = pool.begin().await.map_err(internal)?;

    let row = sqlx::query_as::<_, LinkRow>(
        "SELECT * FROM payment_links WHERE id = $1 AND tenant_id = $2 FOR UPDATE",
    )
    .bind(&id)
    .bind(&tenant)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let mut link: PaymentLink = row.ok_or_else(|| not_found("payment link not found"))?.into();

    if link.status != PaymentStatus::Pending {
        return Err(conflict("link is not awaiting payment"));
    }

    ensure_wallet(&mut tx, &tenant, &link.settlement_currency).await?;
    sqlx::query("UPDATE wallets SET pending = pending + $1 WHERE tenant_id = $2 AND currency = $3")
        .bind(link.settlement_amount)
        .bind(&tenant)
        .bind(&link.settlement_currency)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    sqlx::query(
        "UPDATE platform_revenue SET fx_markup_revenue_usd = fx_markup_revenue_usd + $1, \
         processing_revenue_usd = processing_revenue_usd + $2 WHERE tenant_id = $3",
    )
    .bind(link.fx_markup_revenue_usd)
    .bind(link.processing_fee_revenue_usd)
    .bind(&tenant)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;

    let paid_at = Utc::now();
    sqlx::query("UPDATE payment_links SET status = 'paid', paid_at = $1 WHERE id = $2")
        .bind(paid_at)
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;

    tx.commit().await.map_err(internal)?;
    link.status = PaymentStatus::Paid;
    link.paid_at = Some(paid_at);
    Ok(Json(link))
}

// ---------------------------------------------------------------------------
// Refunds (card reversal or direct debit pull)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct RefundReq {
    pub payment_id: String,
    pub method: RefundMethod,
    pub amount: Option<f64>,
}

pub async fn create_refund(
    State(pool): State<PgPool>,
    auth: AuthCtx,
    Json(req): Json<RefundReq>,
) -> ApiResult<Refund> {
    let tenant = db::resolve_tenant(&pool, &auth).await.map_err(internal)?;
    let mut tx = pool.begin().await.map_err(internal)?;

    let row = sqlx::query_as::<_, LinkRow>(
        "SELECT * FROM payment_links WHERE id = $1 AND tenant_id = $2 FOR UPDATE",
    )
    .bind(&req.payment_id)
    .bind(&tenant)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let link: PaymentLink = row.ok_or_else(|| not_found("payment not found"))?.into();

    if link.status != PaymentStatus::Paid && link.status != PaymentStatus::Settled {
        return Err(conflict("only paid or settled payments can be refunded"));
    }

    let full = link.settlement_amount;
    let amount = req.amount.unwrap_or(full);
    if amount <= 0.0 || amount > full + 0.001 {
        return Err(bad("invalid refund amount"));
    }

    // Draw from available first, then pending.
    let (available, _pending) = sqlx::query_as::<_, (f64, f64)>(
        "SELECT available, pending FROM wallets WHERE tenant_id = $1 AND currency = $2",
    )
    .bind(&tenant)
    .bind(&link.settlement_currency)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    let from_available = amount.min(available);
    let from_pending = amount - from_available;
    sqlx::query(
        "UPDATE wallets SET available = available - $1, pending = pending - $2 \
         WHERE tenant_id = $3 AND currency = $4",
    )
    .bind(from_available)
    .bind(from_pending)
    .bind(&tenant)
    .bind(&link.settlement_currency)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;

    sqlx::query("UPDATE payment_links SET status = 'refunded' WHERE id = $1")
        .bind(&req.payment_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;

    let refund = Refund {
        id: Uuid::new_v4().to_string(),
        payment_id: req.payment_id.clone(),
        payment_reference: link.reference.clone(),
        method: req.method,
        amount: round2(amount),
        currency: link.settlement_currency.clone(),
        status: match req.method {
            RefundMethod::CardReversal => "reversed_via_ryft".to_string(),
            RefundMethod::DirectDebitPull => "pulled_via_ebury_direct_debit".to_string(),
        },
        created_at: Utc::now(),
    };
    sqlx::query(
        "INSERT INTO refunds (id, tenant_id, payment_id, payment_reference, method, amount, currency, status, created_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(&refund.id)
    .bind(&tenant)
    .bind(&refund.payment_id)
    .bind(&refund.payment_reference)
    .bind(refund.method.as_db())
    .bind(refund.amount)
    .bind(&refund.currency)
    .bind(&refund.status)
    .bind(refund.created_at)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;

    tx.commit().await.map_err(internal)?;
    Ok(Json(refund))
}

pub async fn list_refunds(State(pool): State<PgPool>, auth: AuthCtx) -> ApiResult<Vec<Refund>> {
    let tenant = db::resolve_tenant(&pool, &auth).await.map_err(internal)?;
    let rows = sqlx::query_as::<_, RefundRow>(
        "SELECT id, payment_id, payment_reference, method, amount, currency, status, created_at \
         FROM refunds WHERE tenant_id = $1 ORDER BY created_at DESC",
    )
    .bind(&tenant)
    .fetch_all(&pool)
    .await
    .map_err(internal)?;
    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

// ---------------------------------------------------------------------------
// Daily batch settlement
// ---------------------------------------------------------------------------

pub async fn run_settlement(
    State(pool): State<PgPool>,
    auth: AuthCtx,
) -> ApiResult<SettlementBatch> {
    let tenant = db::resolve_tenant(&pool, &auth).await.map_err(internal)?;
    let mut tx = pool.begin().await.map_err(internal)?;

    let pendings = sqlx::query_as::<_, (String, f64)>(
        "SELECT currency, pending FROM wallets WHERE tenant_id = $1 AND pending > 0.0001 ORDER BY position, currency",
    )
    .bind(&tenant)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal)?;
    if pendings.is_empty() {
        return Err(conflict("nothing to settle"));
    }

    let mut lines = Vec::new();
    let mut total_usd = 0.0;
    for (currency, pending) in &pendings {
        let payments: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM payment_links WHERE tenant_id = $1 AND status = 'paid' AND settlement_currency = $2",
        )
        .bind(&tenant)
        .bind(currency)
        .fetch_one(&mut *tx)
        .await
        .map_err(internal)?;
        sqlx::query(
            "UPDATE wallets SET available = available + pending, pending = 0 WHERE tenant_id = $1 AND currency = $2",
        )
        .bind(&tenant)
        .bind(currency)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        total_usd += fx::to_usd(currency, *pending);
        lines.push(SettlementLine {
            currency: currency.clone(),
            amount: round2(*pending),
            payments: payments as usize,
        });
    }

    sqlx::query(
        "UPDATE payment_links SET status = 'settled' WHERE tenant_id = $1 AND status = 'paid'",
    )
    .bind(&tenant)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;

    let batch = SettlementBatch {
        id: format!("BATCH-{}", &Uuid::new_v4().to_string()[..8].to_uppercase()),
        created_at: Utc::now(),
        lines,
        total_usd: round2(total_usd),
    };
    sqlx::query(
        "INSERT INTO settlement_batches (id, tenant_id, created_at, total_usd) VALUES ($1,$2,$3,$4)",
    )
    .bind(&batch.id)
    .bind(&tenant)
    .bind(batch.created_at)
    .bind(batch.total_usd)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    for line in &batch.lines {
        sqlx::query(
            "INSERT INTO settlement_lines (batch_id, currency, amount, payments) VALUES ($1,$2,$3,$4)",
        )
        .bind(&batch.id)
        .bind(&line.currency)
        .bind(line.amount)
        .bind(line.payments as i64)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    }

    tx.commit().await.map_err(internal)?;
    Ok(Json(batch))
}

pub async fn list_batches(
    State(pool): State<PgPool>,
    auth: AuthCtx,
) -> ApiResult<Vec<SettlementBatch>> {
    let tenant = db::resolve_tenant(&pool, &auth).await.map_err(internal)?;
    let batch_rows = sqlx::query_as::<_, (String, DateTime<Utc>, f64)>(
        "SELECT id, created_at, total_usd FROM settlement_batches WHERE tenant_id = $1 ORDER BY created_at DESC",
    )
    .bind(&tenant)
    .fetch_all(&pool)
    .await
    .map_err(internal)?;

    let mut batches = Vec::new();
    for (id, created_at, total_usd) in batch_rows {
        let lines = sqlx::query_as::<_, (String, f64, i64)>(
            "SELECT currency, amount, payments FROM settlement_lines WHERE batch_id = $1 ORDER BY id",
        )
        .bind(&id)
        .fetch_all(&pool)
        .await
        .map_err(internal)?
        .into_iter()
        .map(|(currency, amount, payments)| SettlementLine {
            currency,
            amount,
            payments: payments as usize,
        })
        .collect();
        batches.push(SettlementBatch { id, created_at, lines, total_usd });
    }
    Ok(Json(batches))
}

// ---------------------------------------------------------------------------
// Dashboard summary
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct Counts {
    pub pending: usize,
    pub paid: usize,
    pub settled: usize,
    pub refunded: usize,
    pub total: usize,
}

#[derive(Serialize)]
pub struct DashboardSummary {
    pub operator: Operator,
    pub total_available_usd: f64,
    pub total_pending_usd: f64,
    pub fx_markup_revenue_usd: f64,
    pub processing_revenue_usd: f64,
    pub total_revenue_usd: f64,
    pub processed_volume_usd: f64,
    pub counts: Counts,
}

pub async fn dashboard_summary(
    State(pool): State<PgPool>,
    auth: AuthCtx,
) -> ApiResult<DashboardSummary> {
    let tenant_id = db::resolve_tenant(&pool, &auth).await.map_err(internal)?;
    let tenant = db::get_tenant(&pool, &tenant_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found("tenant not found"))?;
    let operator = Operator {
        id: tenant.id.clone(),
        name: tenant.name.clone(),
        region: tenant.region.clone(),
        settlement_currency: tenant.settlement_currency.clone(),
    };

    let wallet_rows = sqlx::query_as::<_, (String, f64, f64)>(
        "SELECT currency, available, pending FROM wallets WHERE tenant_id = $1",
    )
    .bind(&tenant_id)
    .fetch_all(&pool)
    .await
    .map_err(internal)?;
    let mut total_available_usd = 0.0;
    let mut total_pending_usd = 0.0;
    for (currency, available, pending) in &wallet_rows {
        total_available_usd += fx::to_usd(currency, *available);
        total_pending_usd += fx::to_usd(currency, *pending);
    }

    let link_rows = sqlx::query_as::<_, (String, String, f64)>(
        "SELECT status, settlement_currency, settlement_amount FROM payment_links WHERE tenant_id = $1",
    )
    .bind(&tenant_id)
    .fetch_all(&pool)
    .await
    .map_err(internal)?;
    let mut counts = Counts { pending: 0, paid: 0, settled: 0, refunded: 0, total: 0 };
    let mut processed_volume_usd = 0.0;
    for (status, currency, amount) in &link_rows {
        counts.total += 1;
        match PaymentStatus::parse(status) {
            PaymentStatus::Pending => counts.pending += 1,
            PaymentStatus::Paid => {
                counts.paid += 1;
                processed_volume_usd += fx::to_usd(currency, *amount);
            }
            PaymentStatus::Settled => {
                counts.settled += 1;
                processed_volume_usd += fx::to_usd(currency, *amount);
            }
            PaymentStatus::Refunded => counts.refunded += 1,
        }
    }

    let (fx_rev, proc_rev) = sqlx::query_as::<_, (f64, f64)>(
        "SELECT fx_markup_revenue_usd, processing_revenue_usd FROM platform_revenue WHERE tenant_id = $1",
    )
    .bind(&tenant_id)
    .fetch_optional(&pool)
    .await
    .map_err(internal)?
    .unwrap_or((0.0, 0.0));

    Ok(Json(DashboardSummary {
        operator,
        total_available_usd: round2(total_available_usd),
        total_pending_usd: round2(total_pending_usd),
        fx_markup_revenue_usd: round2(fx_rev),
        processing_revenue_usd: round2(proc_rev),
        total_revenue_usd: round2(fx_rev + proc_rev),
        processed_volume_usd: round2(processed_volume_usd),
        counts,
    }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn ensure_wallet(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: &str,
    currency: &str,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO wallets (tenant_id, currency, available, pending, position) VALUES ($1, $2, 0, 0, 100) \
         ON CONFLICT (tenant_id, currency) DO NOTHING",
    )
    .bind(tenant_id)
    .bind(currency)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(())
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Whether to return magic-link sign-in URLs in the HTTP response (the "mock
/// email" dev convenience). MUST stay off in production — leave `EXPOSE_MAGIC_LINK`
/// unset on the deployed service and read links from the server logs instead.
fn expose_magic_link() -> bool {
    matches!(
        std::env::var("EXPOSE_MAGIC_LINK").as_deref(),
        Ok("true") | Ok("1")
    )
}
