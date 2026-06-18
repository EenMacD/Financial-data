//! Passwordless auth: JWT sessions + the `AuthCtx` request extractor.
//!
//! Sign-in and invites both ride a single opaque-token mechanism (see
//! `auth_tokens` + the handlers). Once a token is consumed we mint a signed
//! JWT carrying the user's id, role, and (for tenant users) their tenant.

use std::env;

use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{header, request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::User;

/// JWT signing secret. Override in production via `JWT_SECRET`.
fn secret() -> String {
    env::var("JWT_SECRET").unwrap_or_else(|_| "dev-insecure-money-data-secret".to_string())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub role: String,
    pub tenant_id: Option<String>,
    pub exp: usize,
}

/// Mint a 7-day session token for `user`.
pub fn issue_jwt(user: &User) -> Result<String, jsonwebtoken::errors::Error> {
    let exp = (Utc::now() + Duration::days(7)).timestamp() as usize;
    let claims = Claims {
        sub: user.id.clone(),
        role: user.role.clone(),
        tenant_id: user.tenant_id.clone(),
        exp,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret().as_bytes()),
    )
}

fn decode_jwt(token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret().as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .map(|data| data.claims)
}

/// Opaque single-use token for magic-link sign-in and invites.
pub fn random_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// Authenticated request context, parsed from the `Authorization: Bearer` JWT.
pub struct AuthCtx {
    pub user_id: String,
    pub role: String,
    /// Tenant baked into the JWT (None for platform admins).
    pub jwt_tenant_id: Option<String>,
    /// Tenant chosen via the `X-Tenant-Id` header (admin tenant switcher).
    pub requested_tenant_id: Option<String>,
}

impl AuthCtx {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }

    /// Tenant the request prefers before DB defaulting: tenant users are pinned
    /// to their JWT tenant; admins follow the `X-Tenant-Id` switcher.
    pub fn preferred_tenant(&self) -> Option<String> {
        if self.role == "tenant" {
            self.jwt_tenant_id.clone()
        } else {
            self.requested_tenant_id.clone()
        }
    }
}

/// 401 rejection for missing/invalid credentials.
pub struct AuthError;

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "authentication required" })),
        )
            .into_response()
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthCtx
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(AuthError)?;
        let claims = decode_jwt(token).map_err(|_| AuthError)?;
        let requested_tenant_id = parts
            .headers
            .get("x-tenant-id")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        Ok(AuthCtx {
            user_id: claims.sub,
            role: claims.role,
            jwt_tenant_id: claims.tenant_id,
            requested_tenant_id,
        })
    }
}
