//! Epic 34.4a: Role-Based Access Control (vault-level).
//!
//! Defines four hierarchical roles and a permission-check helper that
//! extracts the caller's identity from a session token, looks up their
//! vault membership, and compares against the minimum required role.

use crate::db;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use sqlx::SqlitePool;

// ── Roles (hierarchical, highest → lowest) ──────────────────────────

/// Vault-level roles, ordered by privilege (Owner > Admin > Member > Viewer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Viewer = 0,
    Member = 1,
    Admin = 2,
    Owner = 3,
}

impl Role {
    /// Parse a role string from the database.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "owner" => Some(Self::Owner),
            "admin" => Some(Self::Admin),
            "member" => Some(Self::Member),
            "viewer" => Some(Self::Viewer),
            _ => None,
        }
    }

    /// The role satisfies `required` if its privilege level is >= required.
    pub fn satisfies(self, required: Role) -> bool {
        (self as u8) >= (required as u8)
    }
}

// ── Caller identity resolved from session ───────────────────────────

/// Successfully authenticated + authorized caller.
pub struct AuthorizedCaller {
    pub user_id: String,
    pub device_id: String,
    pub vault_id: String,
    pub role: Role,
}

// ── Main ACL check ──────────────────────────────────────────────────

/// Authenticate the request via session token AND authorize against a
/// minimum vault role.  Returns `AuthorizedCaller` on success or an
/// axum error response (401 / 403 / 500) on failure.
pub async fn require_role(
    pool: &SqlitePool,
    headers: &HeaderMap,
    min_role: Role,
) -> Result<AuthorizedCaller, axum::response::Response> {
    // 1. Extract & validate session token
    let session = extract_session_or_401(pool, headers).await?;

    // 2. Resolve vault_id
    let vault_id = get_vault_id_or_500(pool).await?;

    // 3. Look up vault membership
    let member = db::get_vault_member(pool, &session.user_id, &vault_id)
        .await
        .map_err(|e| internal_err(e))?
        .ok_or_else(|| forbidden("user is not a vault member"))?;

    let role = Role::from_str(&member.role)
        .ok_or_else(|| internal_err_msg(&format!("unknown role: {}", member.role)))?;

    // 4. Check privilege
    if !role.satisfies(min_role) {
        return Err(forbidden(&format!(
            "requires {:?} or higher, caller is {:?}",
            min_role, role
        )));
    }

    Ok(AuthorizedCaller {
        user_id: session.user_id,
        device_id: session.device_id,
        vault_id,
        role,
    })
}

/// Like `require_role` but only authenticates (any valid session, no
/// vault membership required).  Used for endpoints that don't need
/// role-based authorization (e.g. health checks with auth).
pub async fn require_session(
    pool: &SqlitePool,
    headers: &HeaderMap,
) -> Result<db::UserSession, axum::response::Response> {
    extract_session_or_401(pool, headers).await
}

// ── Internal helpers ────────────────────────────────────────────────

async fn extract_session_or_401(
    pool: &SqlitePool,
    headers: &HeaderMap,
) -> Result<db::UserSession, axum::response::Response> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| unauthorized("missing or malformed Authorization header"))?;

    db::validate_user_session(pool, auth)
        .await
        .map_err(|e| internal_err(e))?
        .ok_or_else(|| unauthorized("invalid or expired session token"))
}

async fn get_vault_id_or_500(pool: &SqlitePool) -> Result<String, axum::response::Response> {
    match db::get_vault_params(pool).await {
        Ok(Some(v)) => Ok(v.vault_id),
        Ok(None) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "vault_not_initialized" })),
        )
            .into_response()),
        Err(e) => Err(internal_err(e)),
    }
}

fn unauthorized(msg: &str) -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({
            "error": "unauthorized",
            "message": msg,
        })),
    )
        .into_response()
}

fn forbidden(msg: &str) -> axum::response::Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({
            "error": "insufficient_permissions",
            "message": msg,
        })),
    )
        .into_response()
}

fn internal_err(e: impl std::fmt::Display) -> axum::response::Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "error": "internal_error",
            "message": e.to_string(),
        })),
    )
        .into_response()
}

fn internal_err_msg(msg: &str) -> axum::response::Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "error": "internal_error",
            "message": msg,
        })),
    )
        .into_response()
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_hierarchy() {
        assert!(Role::Owner.satisfies(Role::Owner));
        assert!(Role::Owner.satisfies(Role::Admin));
        assert!(Role::Owner.satisfies(Role::Member));
        assert!(Role::Owner.satisfies(Role::Viewer));

        assert!(!Role::Admin.satisfies(Role::Owner));
        assert!(Role::Admin.satisfies(Role::Admin));
        assert!(Role::Admin.satisfies(Role::Member));
        assert!(Role::Admin.satisfies(Role::Viewer));

        assert!(!Role::Member.satisfies(Role::Owner));
        assert!(!Role::Member.satisfies(Role::Admin));
        assert!(Role::Member.satisfies(Role::Member));
        assert!(Role::Member.satisfies(Role::Viewer));

        assert!(!Role::Viewer.satisfies(Role::Owner));
        assert!(!Role::Viewer.satisfies(Role::Admin));
        assert!(!Role::Viewer.satisfies(Role::Member));
        assert!(Role::Viewer.satisfies(Role::Viewer));
    }

    #[test]
    fn role_from_str() {
        assert_eq!(Role::from_str("owner"), Some(Role::Owner));
        assert_eq!(Role::from_str("admin"), Some(Role::Admin));
        assert_eq!(Role::from_str("member"), Some(Role::Member));
        assert_eq!(Role::from_str("viewer"), Some(Role::Viewer));
        assert_eq!(Role::from_str("superadmin"), None);
        assert_eq!(Role::from_str(""), None);
    }

    #[tokio::test]
    async fn require_role_rejects_missing_header() {
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let headers = HeaderMap::new();
        let result = require_role(&pool, &headers, Role::Viewer).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn require_role_rejects_invalid_token() {
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer bogus-token".parse().unwrap());
        let result = require_role(&pool, &headers, Role::Viewer).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn require_role_enforces_hierarchy() {
        let pool = db::init_db("sqlite::memory:").await.unwrap();

        // Set up: vault, user, device, session, membership
        db::set_vault_params(&pool, b"salt1234567890ab", "argon2id", "vault-1")
            .await
            .unwrap();
        db::create_user(&pool, "u-member", "Bob", None, "local", None)
            .await
            .unwrap();
        db::create_device(&pool, "dev-b", "u-member", "BobPC", &[0u8; 32])
            .await
            .unwrap();
        db::add_vault_member(&pool, "u-member", "vault-1", "member", None)
            .await
            .unwrap();
        let token = db::generate_session_token();
        db::create_user_session(&pool, &token, "u-member", "dev-b", db::SESSION_TTL_SECONDS)
            .await
            .unwrap();

        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {token}").parse().unwrap(),
        );

        // Member can access Viewer-level endpoint
        assert!(require_role(&pool, &headers, Role::Viewer).await.is_ok());

        // Member can access Member-level endpoint
        assert!(require_role(&pool, &headers, Role::Member).await.is_ok());

        // Member CANNOT access Admin-level endpoint
        assert!(require_role(&pool, &headers, Role::Admin).await.is_err());

        // Member CANNOT access Owner-level endpoint
        assert!(require_role(&pool, &headers, Role::Owner).await.is_err());
    }

    #[tokio::test]
    async fn require_role_owner_can_do_everything() {
        let pool = db::init_db("sqlite::memory:").await.unwrap();

        db::set_vault_params(&pool, b"salt1234567890ab", "argon2id", "vault-1")
            .await
            .unwrap();
        db::create_user(&pool, "u-owner", "Alice", None, "local", None)
            .await
            .unwrap();
        db::create_device(&pool, "dev-a", "u-owner", "AlicePC", &[0u8; 32])
            .await
            .unwrap();
        db::add_vault_member(&pool, "u-owner", "vault-1", "owner", None)
            .await
            .unwrap();
        let token = db::generate_session_token();
        db::create_user_session(&pool, &token, "u-owner", "dev-a", db::SESSION_TTL_SECONDS)
            .await
            .unwrap();

        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {token}").parse().unwrap(),
        );

        assert!(require_role(&pool, &headers, Role::Viewer).await.is_ok());
        assert!(require_role(&pool, &headers, Role::Member).await.is_ok());
        assert!(require_role(&pool, &headers, Role::Admin).await.is_ok());
        assert!(require_role(&pool, &headers, Role::Owner).await.is_ok());
    }

    #[tokio::test]
    async fn require_role_non_member_gets_403() {
        let pool = db::init_db("sqlite::memory:").await.unwrap();

        db::set_vault_params(&pool, b"salt1234567890ab", "argon2id", "vault-1")
            .await
            .unwrap();
        db::create_user(&pool, "u-stranger", "Eve", None, "local", None)
            .await
            .unwrap();
        db::create_device(&pool, "dev-s", "u-stranger", "EvePC", &[0u8; 32])
            .await
            .unwrap();
        // NOTE: no vault_members entry for u-stranger
        let token = db::generate_session_token();
        db::create_user_session(&pool, &token, "u-stranger", "dev-s", db::SESSION_TTL_SECONDS)
            .await
            .unwrap();

        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {token}").parse().unwrap(),
        );

        // Even Viewer-level access is denied for non-members
        assert!(require_role(&pool, &headers, Role::Viewer).await.is_err());
    }
}
