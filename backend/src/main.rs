//! money-data: B2B payment orchestration platform.
//!
//! Sits between Ebury (FX + multi-currency wallets) and Ecommpay (card
//! gateway). Generates currency-converted payment links, earns a 2% FX
//! markup + 1% processing fee, settles into operator wallets via a daily
//! batch, and serves the merchant dashboard's data.

mod auth;
mod db;
mod email;
mod fx;
mod handlers;
mod models;
mod ryft;

use std::env;

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    // Load backend/.env if present (local/staging). dotenvy does NOT override vars
    // already set in the real environment, so deployed config always wins.
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Refuse to boot without a real signing secret — otherwise session tokens
    // would be forgeable against the insecure dev default. Generate one with
    // `openssl rand -hex 32`. (start.sh sets a throwaway value for local dev.)
    if env::var("JWT_SECRET").map(|s| s.trim().is_empty()).unwrap_or(true) {
        panic!(
            "JWT_SECRET must be set (e.g. `openssl rand -hex 32`); refusing to start \
             with an insecure default"
        );
    }

    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set (Postgres / Neon connection string)");
    let pool = db::connect_and_init(&database_url)
        .await
        .expect("failed to connect to / initialise the database");

    // Bootstrap/refresh the platform admin from env so a fresh OR already-seeded
    // database always has a known email+password login. No-op unless both vars are
    // set; changing ADMIN_PASSWORD and restarting rotates the password.
    if let (Ok(email), Ok(password)) = (env::var("ADMIN_EMAIL"), env::var("ADMIN_PASSWORD")) {
        let email = email.trim().to_lowercase();
        if !email.is_empty() && !password.trim().is_empty() {
            let hash = bcrypt::hash(&password, bcrypt::DEFAULT_COST).expect("hash admin password");
            db::ensure_admin(&pool, &email, "Admin", &hash)
                .await
                .expect("ensure admin account");
            tracing::info!("ensured admin login for {email}");
        }
    }

    // No CORS layer: the container serves the frontend and the API on the same
    // origin (and in dev the Vite server proxies /api), so cross-origin access
    // is never needed. If a separate frontend origin is ever introduced, add a
    // *scoped* CorsLayer here rather than allowing `Any`.
    let app = Router::new()
        .route("/api/health", get(handlers::health))
        .route("/api/rates", get(handlers::list_rates))
        // Auth (public) — email+password sign-in, plus the (dormant) passwordless
        // magic link + invite acceptance.
        .route("/api/auth/login", post(handlers::login))
        .route("/api/auth/magic-link", post(handlers::request_magic_link))
        .route("/api/auth/verify", post(handlers::verify_magic_link))
        .route("/api/invites/accept", post(handlers::accept_invite))
        // Public self-serve KYB/KYC sign-up.
        .route("/api/merchants", post(handlers::signup_merchant))
        // Session / tenancy (auth required via AuthCtx extractor).
        .route("/api/me", get(handlers::me))
        .route(
            "/api/tenants",
            get(handlers::list_tenants).post(handlers::create_tenant),
        )
        .route(
            "/api/tenants/:tenant_id/signin-link",
            post(handlers::send_tenant_signin_link),
        )
        .route("/api/invites", post(handlers::create_invite))
        .route(
            "/api/users",
            get(handlers::list_users).post(handlers::create_user),
        )
        // Tenant-scoped dashboard data.
        .route("/api/wallets", get(handlers::list_wallets))
        .route("/api/quote", post(handlers::create_quote))
        .route(
            "/api/payment-links",
            get(handlers::list_links).post(handlers::create_link),
        )
        .route("/api/payment-links/:id/pay", post(handlers::pay_link))
        .route(
            "/api/refunds",
            get(handlers::list_refunds).post(handlers::create_refund),
        )
        .route("/api/settlement/run", post(handlers::run_settlement))
        .route("/api/settlement/batches", get(handlers::list_batches))
        .route("/api/dashboard", get(handlers::dashboard_summary))
        .fallback_service(static_files())
        .with_state(pool);

    // Bind 0.0.0.0 and honour $PORT so the same binary runs in a container.
    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("money-data backend listening on http://{addr}");
    axum::serve(listener, app).await.unwrap();
}

/// Serve the built Vite frontend (bundled into the container) with an SPA
/// fallback to index.html. `FRONTEND_DIR` defaults to `frontend_dist`; during
/// local dev the Vite server is used instead, so a missing directory simply
/// 404s non-API routes — harmless.
fn static_files() -> ServeDir<ServeFile> {
    let dir = env::var("FRONTEND_DIR").unwrap_or_else(|_| "frontend_dist".to_string());
    let index = format!("{dir}/index.html");
    // `.fallback` serves index.html with a 200 (proper SPA behaviour); the
    // `.not_found_service` variant would force a 404 status.
    ServeDir::new(dir).fallback(ServeFile::new(index))
}
