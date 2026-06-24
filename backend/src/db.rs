//! Postgres access: pool setup, migrations, demo seeding, and shared helpers.

use chrono::{DateTime, Duration, Utc};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::AuthCtx;
use crate::fx;
use crate::models::*;
use crate::ryft;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Connect, run migrations, and seed demo data on a fresh database.
pub async fn connect_and_init(database_url: &str) -> Result<PgPool, BoxError> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    let tenants: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tenants")
        .fetch_one(&pool)
        .await?;
    if tenants == 0 {
        seed(&pool).await?;
        tracing::info!("seeded demo data");
    }

    Ok(pool)
}

// ---------------------------------------------------------------------------
// Payment links
// ---------------------------------------------------------------------------

/// Build a payment link record (not yet persisted) from a quote, minting the
/// backing Ryft hosted payment session.
pub fn build_link(
    tenant_id: &str,
    guest_name: String,
    description: String,
    settlement_currency: &str,
    settlement_amount: f64,
    presentment_currency: &str,
    trip_id: Option<String>,
) -> Result<PaymentLink, String> {
    let q = fx::quote(settlement_currency, settlement_amount, presentment_currency)?;
    let id = Uuid::new_v4().to_string();
    let reference = format!("PL-{}", &id[..8].to_uppercase());
    let session = ryft::create_payment_session(
        q.total_charged,
        &q.presentment_currency,
        trip_id.as_deref(),
        None,
    );
    Ok(PaymentLink {
        id,
        tenant_id: tenant_id.to_string(),
        reference,
        trip_id,
        guest_name,
        description,
        settlement_currency: q.settlement_currency,
        settlement_amount: q.settlement_amount,
        presentment_currency: q.presentment_currency,
        mid_rate: q.mid_rate,
        quoted_rate: q.quoted_rate,
        converted_amount: q.converted_amount,
        processing_fee: q.processing_fee,
        total_charged: q.total_charged,
        fx_markup_revenue_usd: q.fx_markup_revenue_usd,
        processing_fee_revenue_usd: q.processing_fee_revenue_usd,
        status: PaymentStatus::Pending,
        url: session.hosted_url,
        ryft_session_id: Some(session.session_id),
        created_at: Utc::now(),
        paid_at: None,
    })
}

pub async fn insert_link(pool: &PgPool, l: &PaymentLink) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO payment_links (id, tenant_id, reference, trip_id, guest_name, description, \
         settlement_currency, settlement_amount, presentment_currency, mid_rate, quoted_rate, \
         converted_amount, processing_fee, total_charged, fx_markup_revenue_usd, \
         processing_fee_revenue_usd, status, url, ryft_session_id, created_at, paid_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21)",
    )
    .bind(&l.id)
    .bind(&l.tenant_id)
    .bind(&l.reference)
    .bind(&l.trip_id)
    .bind(&l.guest_name)
    .bind(&l.description)
    .bind(&l.settlement_currency)
    .bind(l.settlement_amount)
    .bind(&l.presentment_currency)
    .bind(l.mid_rate)
    .bind(l.quoted_rate)
    .bind(l.converted_amount)
    .bind(l.processing_fee)
    .bind(l.total_charged)
    .bind(l.fx_markup_revenue_usd)
    .bind(l.processing_fee_revenue_usd)
    .bind(l.status.as_db())
    .bind(&l.url)
    .bind(&l.ryft_session_id)
    .bind(l.created_at)
    .bind(l.paid_at)
    .execute(pool)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tenants
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct TenantRow {
    id: String,
    name: String,
    region: String,
    settlement_currency: String,
    registration_number: Option<String>,
    country: Option<String>,
    contact_name: Option<String>,
    contact_email: Option<String>,
    ryft_subaccount_id: Option<String>,
    onboarding_status: String,
    onboarding_url: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<TenantRow> for Tenant {
    fn from(r: TenantRow) -> Self {
        Tenant {
            id: r.id,
            name: r.name,
            region: r.region,
            settlement_currency: r.settlement_currency,
            registration_number: r.registration_number,
            country: r.country,
            contact_name: r.contact_name,
            contact_email: r.contact_email,
            ryft_subaccount_id: r.ryft_subaccount_id,
            onboarding_status: r.onboarding_status,
            onboarding_url: r.onboarding_url,
            created_at: r.created_at,
        }
    }
}

const TENANT_COLS: &str = "id, name, region, settlement_currency, registration_number, country, \
     contact_name, contact_email, ryft_subaccount_id, onboarding_status, onboarding_url, created_at";

/// Create a tenant, begin Ryft KYB/KYC onboarding, and provision its wallets +
/// revenue row. Shared by admin tenant-create and public self-serve sign-up.
#[allow(clippy::too_many_arguments)]
pub async fn create_tenant_with_setup(
    pool: &PgPool,
    name: &str,
    region: &str,
    settlement_currency: &str,
    registration_number: Option<String>,
    country: Option<String>,
    contact_name: Option<String>,
    contact_email: Option<String>,
) -> Result<Tenant, sqlx::Error> {
    let id = format!("tnt_{}", &Uuid::new_v4().simple().to_string()[..12]);
    let onboarding = ryft::create_subaccount_onboarding(name);
    let tenant = Tenant {
        id: id.clone(),
        name: name.to_string(),
        region: region.to_string(),
        settlement_currency: settlement_currency.to_uppercase(),
        registration_number,
        country,
        contact_name,
        contact_email,
        ryft_subaccount_id: Some(onboarding.subaccount_id),
        onboarding_status: onboarding.status,
        onboarding_url: Some(onboarding.onboarding_url),
        created_at: Utc::now(),
    };
    sqlx::query(
        "INSERT INTO tenants (id, name, region, settlement_currency, registration_number, country, \
         contact_name, contact_email, ryft_subaccount_id, onboarding_status, onboarding_url, \
         created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind(&tenant.id)
    .bind(&tenant.name)
    .bind(&tenant.region)
    .bind(&tenant.settlement_currency)
    .bind(&tenant.registration_number)
    .bind(&tenant.country)
    .bind(&tenant.contact_name)
    .bind(&tenant.contact_email)
    .bind(&tenant.ryft_subaccount_id)
    .bind(&tenant.onboarding_status)
    .bind(&tenant.onboarding_url)
    .bind(tenant.created_at)
    .execute(pool)
    .await?;
    provision_tenant_finance(pool, &tenant.id, &tenant.settlement_currency).await?;
    Ok(tenant)
}

/// Wallets (USD/EUR/GBP + settlement ccy) and a revenue row for a new tenant.
async fn provision_tenant_finance(
    pool: &PgPool,
    tenant_id: &str,
    settlement_currency: &str,
) -> Result<(), sqlx::Error> {
    let mut currencies = vec!["USD".to_string(), "EUR".to_string(), "GBP".to_string()];
    let sc = settlement_currency.to_uppercase();
    if !currencies.contains(&sc) {
        currencies.push(sc);
    }
    for (i, c) in currencies.iter().enumerate() {
        sqlx::query(
            "INSERT INTO wallets (tenant_id, currency, available, pending, position) \
             VALUES ($1,$2,0,0,$3) ON CONFLICT DO NOTHING",
        )
        .bind(tenant_id)
        .bind(c)
        .bind(i as i32)
        .execute(pool)
        .await?;
    }
    sqlx::query(
        "INSERT INTO platform_revenue (tenant_id, fx_markup_revenue_usd, processing_revenue_usd) \
         VALUES ($1,0,0) ON CONFLICT DO NOTHING",
    )
    .bind(tenant_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_tenants(pool: &PgPool) -> Result<Vec<Tenant>, sqlx::Error> {
    let rows = sqlx::query_as::<_, TenantRow>(&format!(
        "SELECT {TENANT_COLS} FROM tenants ORDER BY created_at"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn get_tenant(pool: &PgPool, id: &str) -> Result<Option<Tenant>, sqlx::Error> {
    let row = sqlx::query_as::<_, TenantRow>(&format!(
        "SELECT {TENANT_COLS} FROM tenants WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

/// Resolve the tenant a request operates on. Tenant users are pinned to their
/// own tenant; admins follow the `X-Tenant-Id` switcher, defaulting to the
/// first tenant when none is selected.
pub async fn resolve_tenant(pool: &PgPool, auth: &AuthCtx) -> Result<String, sqlx::Error> {
    if let Some(t) = auth.preferred_tenant() {
        return Ok(t);
    }
    let first: Option<String> =
        sqlx::query_scalar("SELECT id FROM tenants ORDER BY created_at LIMIT 1")
            .fetch_optional(pool)
            .await?;
    Ok(first.unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Users
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct UserRow {
    id: String,
    email: String,
    name: String,
    role: String,
    tenant_id: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<UserRow> for User {
    fn from(r: UserRow) -> Self {
        User {
            id: r.id,
            email: r.email,
            name: r.name,
            role: r.role,
            tenant_id: r.tenant_id,
            created_at: r.created_at,
        }
    }
}

const USER_COLS: &str = "id, email, name, role, tenant_id, created_at";

pub async fn find_user_by_email(pool: &PgPool, email: &str) -> Result<Option<User>, sqlx::Error> {
    let row = sqlx::query_as::<_, UserRow>(&format!(
        "SELECT {USER_COLS} FROM users WHERE lower(email) = lower($1)"
    ))
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into))
}

pub async fn get_user(pool: &PgPool, id: &str) -> Result<Option<User>, sqlx::Error> {
    let row = sqlx::query_as::<_, UserRow>(&format!("SELECT {USER_COLS} FROM users WHERE id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(Into::into))
}

pub async fn list_tenant_users(pool: &PgPool, tenant_id: &str) -> Result<Vec<User>, sqlx::Error> {
    let rows = sqlx::query_as::<_, UserRow>(&format!(
        "SELECT {USER_COLS} FROM users WHERE tenant_id = $1 ORDER BY created_at"
    ))
    .bind(tenant_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

/// Return the existing user for `email`, or create one with the given role/tenant.
pub async fn upsert_user(
    pool: &PgPool,
    email: &str,
    name: &str,
    role: &str,
    tenant_id: Option<&str>,
) -> Result<User, sqlx::Error> {
    if let Some(u) = find_user_by_email(pool, email).await? {
        return Ok(u);
    }
    let user = User {
        id: format!("usr_{}", &Uuid::new_v4().simple().to_string()[..12]),
        email: email.to_string(),
        name: name.to_string(),
        role: role.to_string(),
        tenant_id: tenant_id.map(|s| s.to_string()),
        created_at: Utc::now(),
    };
    sqlx::query(
        "INSERT INTO users (id, email, name, role, tenant_id, created_at) \
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(&user.id)
    .bind(&user.email)
    .bind(&user.name)
    .bind(&user.role)
    .bind(&user.tenant_id)
    .bind(user.created_at)
    .execute(pool)
    .await?;
    Ok(user)
}

/// The bcrypt hash stored for `email`, if any. `None` means either no such user
/// or a user that has never had a password set (cannot log in with one).
pub async fn get_password_hash(pool: &PgPool, email: &str) -> Result<Option<String>, sqlx::Error> {
    let hash: Option<Option<String>> =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE lower(email) = lower($1)")
            .bind(email)
            .fetch_optional(pool)
            .await?;
    Ok(hash.flatten())
}

/// Create a user with a password hash (the "Add user" flow). Caller must have
/// already checked the email is free; the UNIQUE(email) constraint is the backstop.
pub async fn create_user_with_password(
    pool: &PgPool,
    email: &str,
    name: &str,
    role: &str,
    tenant_id: Option<&str>,
    password_hash: &str,
) -> Result<User, sqlx::Error> {
    let user = User {
        id: format!("usr_{}", &Uuid::new_v4().simple().to_string()[..12]),
        email: email.to_string(),
        name: name.to_string(),
        role: role.to_string(),
        tenant_id: tenant_id.map(|s| s.to_string()),
        created_at: Utc::now(),
    };
    sqlx::query(
        "INSERT INTO users (id, email, name, role, tenant_id, password_hash, created_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(&user.id)
    .bind(&user.email)
    .bind(&user.name)
    .bind(&user.role)
    .bind(&user.tenant_id)
    .bind(password_hash)
    .bind(user.created_at)
    .execute(pool)
    .await?;
    Ok(user)
}

/// Idempotently ensure a platform-admin account exists with this password hash.
/// Runs every boot from ADMIN_EMAIL/ADMIN_PASSWORD so a fresh or already-seeded
/// database always has a known login (and lets the operator rotate the password
/// by changing the env var). Upserts on the email's UNIQUE constraint.
pub async fn ensure_admin(
    pool: &PgPool,
    email: &str,
    name: &str,
    password_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO users (id, email, name, role, tenant_id, password_hash, created_at) \
         VALUES ($1, lower($2), $3, 'admin', NULL, $4, now()) \
         ON CONFLICT (email) DO UPDATE \
           SET password_hash = EXCLUDED.password_hash, role = 'admin'",
    )
    .bind(format!("usr_{}", &Uuid::new_v4().simple().to_string()[..12]))
    .bind(email)
    .bind(name)
    .bind(password_hash)
    .execute(pool)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Auth tokens (magic-link sign-in + invites)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub async fn create_auth_token(
    pool: &PgPool,
    token: &str,
    kind: &str,
    email: &str,
    name: Option<&str>,
    role: Option<&str>,
    tenant_id: Option<&str>,
    expires_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO auth_tokens (token, kind, email, name, role, tenant_id, expires_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(token)
    .bind(kind)
    .bind(email)
    .bind(name)
    .bind(role)
    .bind(tenant_id)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub struct ConsumedToken {
    pub email: String,
    pub role: Option<String>,
    pub tenant_id: Option<String>,
}

/// Atomically mark a valid, unused, unexpired token of `kind` as used and
/// return its payload. `None` if it doesn't exist / was already used / expired.
pub async fn consume_token(
    pool: &PgPool,
    token: &str,
    kind: &str,
) -> Result<Option<ConsumedToken>, sqlx::Error> {
    let row = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
        "UPDATE auth_tokens SET used_at = now() \
         WHERE token = $1 AND kind = $2 AND used_at IS NULL AND expires_at > now() \
         RETURNING email, role, tenant_id",
    )
    .bind(token)
    .bind(kind)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(email, role, tenant_id)| ConsumedToken {
        email,
        role,
        tenant_id,
    }))
}

// ---------------------------------------------------------------------------
// Demo seed
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn insert_tenant_row(
    pool: &PgPool,
    id: &str,
    name: &str,
    region: &str,
    settlement_currency: &str,
    registration_number: &str,
    country: &str,
    contact_name: &str,
    contact_email: &str,
    onboarding_status: &str,
) -> Result<(), sqlx::Error> {
    let onboarding = ryft::create_subaccount_onboarding(name);
    sqlx::query(
        "INSERT INTO tenants (id, name, region, settlement_currency, registration_number, country, \
         contact_name, contact_email, ryft_subaccount_id, onboarding_status, onboarding_url, \
         created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind(id)
    .bind(name)
    .bind(region)
    .bind(settlement_currency)
    .bind(registration_number)
    .bind(country)
    .bind(contact_name)
    .bind(contact_email)
    .bind(&onboarding.subaccount_id)
    .bind(onboarding_status)
    .bind(&onboarding.onboarding_url)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

async fn insert_user_row(
    pool: &PgPool,
    email: &str,
    name: &str,
    role: &str,
    tenant_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO users (id, email, name, role, tenant_id, created_at) VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(format!("usr_{}", &Uuid::new_v4().simple().to_string()[..12]))
    .bind(email)
    .bind(name)
    .bind(role)
    .bind(tenant_id)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

async fn seed(pool: &PgPool) -> Result<(), sqlx::Error> {
    // Tenant 1: fully-onboarded demo operator with payment history.
    insert_tenant_row(
        pool,
        "tnt_savannah",
        "Savannah Trails Safari Co.",
        "Nairobi, Kenya",
        "USD",
        "CRP-2019-114829",
        "Kenya",
        "Amara Otieno",
        "ops@savannahtrails.example",
        "verified",
    )
    .await?;
    // Tenant 2: mid-onboarding, no history — shows the tenant switcher +
    // onboarding states in the admin portal.
    insert_tenant_row(
        pool,
        "tnt_okavango",
        "Okavango Expeditions",
        "Maun, Botswana",
        "USD",
        "BW-CO-77120",
        "Botswana",
        "Tebogo Khama",
        "hello@okavango.example",
        "in_review",
    )
    .await?;
    provision_tenant_finance(pool, "tnt_okavango", "USD").await?;

    // A platform admin (the demo login) + a tenant user for Savannah Trails.
    insert_user_row(pool, "digitallymba@gmail.com", "Platform Admin", "admin", None).await?;
    insert_user_row(
        pool,
        "owner@savannahtrails.example",
        "Amara Otieno",
        "tenant",
        Some("tnt_savannah"),
    )
    .await?;

    // Savannah wallets (incl. ZAR, used by a sample) + revenue row.
    for (i, c) in ["USD", "EUR", "GBP", "ZAR"].iter().enumerate() {
        sqlx::query(
            "INSERT INTO wallets (tenant_id, currency, available, pending, position) \
             VALUES ('tnt_savannah',$1,0,0,$2)",
        )
        .bind(c)
        .bind(i as i32)
        .execute(pool)
        .await?;
    }
    sqlx::query(
        "INSERT INTO platform_revenue (tenant_id, fx_markup_revenue_usd, processing_revenue_usd) \
         VALUES ('tnt_savannah',0,0)",
    )
    .execute(pool)
    .await?;

    // (trip_id, guest, description, settlement ccy, amount, presentment ccy, days_ago, status)
    let samples: [(&str, &str, &str, &str, f64, &str, i64, PaymentStatus); 7] = [
        ("TRIP-MM-2041", "Emma Thompson", "10-day Masai Mara photographic safari", "USD", 8400.0, "GBP", 6, PaymentStatus::Settled),
        ("TRIP-SG-2042", "Lukas Müller", "Serengeti migration package (2 guests)", "USD", 12600.0, "EUR", 5, PaymentStatus::Settled),
        ("TRIP-AM-2043", "Sophie Dubois", "Amboseli elephant lodge, 4 nights", "EUR", 3200.0, "EUR", 4, PaymentStatus::Settled),
        ("TRIP-KM-2044", "James Carter", "Kilimanjaro Machame route trek", "USD", 4900.0, "USD", 3, PaymentStatus::Paid),
        ("TRIP-CT-2045", "Olivia Smith", "Cape Town & winelands extension", "ZAR", 48000.0, "GBP", 2, PaymentStatus::Paid),
        ("TRIP-DB-2046", "Marco Rossi", "Diani Beach honeymoon, 7 nights", "USD", 5600.0, "EUR", 1, PaymentStatus::Paid),
        ("TRIP-NG-2047", "Hannah Berg", "Ngorongoro Crater day safari", "USD", 1450.0, "AUD", 0, PaymentStatus::Pending),
    ];

    let mut fx_rev = 0.0;
    let mut proc_rev = 0.0;
    for (trip, guest, desc, scur, amt, pcur, days_ago, status) in samples {
        let mut link = build_link(
            "tnt_savannah",
            guest.to_string(),
            desc.to_string(),
            scur,
            amt,
            pcur,
            Some(trip.to_string()),
        )
        .expect("seed quote");
        link.created_at = Utc::now() - Duration::days(days_ago);
        link.status = status;

        match status {
            PaymentStatus::Settled => {
                link.paid_at = Some(link.created_at + Duration::hours(2));
                insert_link(pool, &link).await?;
                sqlx::query(
                    "UPDATE wallets SET available = available + $1 WHERE tenant_id = 'tnt_savannah' AND currency = $2",
                )
                .bind(amt)
                .bind(scur)
                .execute(pool)
                .await?;
                fx_rev += link.fx_markup_revenue_usd;
                proc_rev += link.processing_fee_revenue_usd;
            }
            PaymentStatus::Paid => {
                link.paid_at = Some(link.created_at + Duration::hours(1));
                insert_link(pool, &link).await?;
                sqlx::query(
                    "UPDATE wallets SET pending = pending + $1 WHERE tenant_id = 'tnt_savannah' AND currency = $2",
                )
                .bind(amt)
                .bind(scur)
                .execute(pool)
                .await?;
                fx_rev += link.fx_markup_revenue_usd;
                proc_rev += link.processing_fee_revenue_usd;
            }
            _ => {
                insert_link(pool, &link).await?;
            }
        }
    }

    sqlx::query(
        "UPDATE platform_revenue SET fx_markup_revenue_usd = $1, processing_revenue_usd = $2 WHERE tenant_id = 'tnt_savannah'",
    )
    .bind(fx_rev)
    .bind(proc_rev)
    .execute(pool)
    .await?;

    // A historical settlement batch covering the already-settled funds.
    let usd_settled = 8400.0 + 12600.0;
    let eur_settled = 3200.0;
    let batch_id = format!("BATCH-{}", &Uuid::new_v4().to_string()[..8].to_uppercase());
    let total_usd = fx::to_usd("USD", usd_settled) + fx::to_usd("EUR", eur_settled);
    sqlx::query(
        "INSERT INTO settlement_batches (id, tenant_id, created_at, total_usd) VALUES ($1,'tnt_savannah',$2,$3)",
    )
    .bind(&batch_id)
    .bind(Utc::now() - Duration::days(3))
    .bind(total_usd)
    .execute(pool)
    .await?;
    for (ccy, amount, payments) in [("USD", usd_settled, 2i64), ("EUR", eur_settled, 1i64)] {
        sqlx::query(
            "INSERT INTO settlement_lines (batch_id, currency, amount, payments) VALUES ($1,$2,$3,$4)",
        )
        .bind(&batch_id)
        .bind(ccy)
        .bind(amount)
        .bind(payments)
        .execute(pool)
        .await?;
    }

    Ok(())
}
