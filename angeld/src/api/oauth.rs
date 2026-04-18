use super::error::ApiError;
use super::ApiState;
use crate::config::AppConfig;
use crate::db;

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Redirect};
use axum::routing::get;
use axum::Router;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::warn;

// ── PKCE helpers ──────────────────────────────────────────────────────

fn pkce_pair() -> (String, String) {
    use rand::RngCore;
    let mut bytes = [0u8; 96];
    rand::thread_rng().fill_bytes(&mut bytes);
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let digest = Sha256::digest(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    (verifier, challenge)
}

// ── GET /api/auth/google/start ────────────────────────────────────────

async fn get_google_start(
    State(state): State<ApiState>,
) -> Result<impl IntoResponse, ApiError> {
    let cfg = AppConfig::from_env();
    let client_id = cfg.google_client_id.ok_or(ApiError::NotFound {
        resource: "oauth",
        id: "google_client_id_not_configured".to_string(),
    })?;

    let (pkce_verifier, pkce_challenge) = pkce_pair();
    let oauth_state = db::new_user_id();

    db::delete_expired_oauth_states(&state.pool).await.ok();
    db::create_oauth_state(&state.pool, &oauth_state, &pkce_verifier, 600).await?;

    let url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}\
         &code_challenge={}&code_challenge_method=S256\
         &access_type=offline&prompt=consent",
        cfg.oauth_google_auth_url,
        urlencoding::encode(&client_id),
        urlencoding::encode(&cfg.oauth_redirect_url),
        urlencoding::encode("openid email profile"),
        urlencoding::encode(&oauth_state),
        urlencoding::encode(&pkce_challenge),
    );

    Ok(Redirect::temporary(&url))
}

// ── GET /api/auth/google/callback ─────────────────────────────────────

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct UserinfoResponse {
    sub: String,
    email: Option<String>,
    name: Option<String>,
}

async fn get_google_callback(
    State(state): State<ApiState>,
    Query(params): Query<CallbackQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = params.error {
        return Err(ApiError::BadRequest {
            code: "oauth_error",
            message: err,
        });
    }

    let code = params.code.ok_or(ApiError::BadRequest {
        code: "missing_code",
        message: "OAuth callback missing `code` parameter".to_string(),
    })?;
    let oauth_state = params.state.ok_or(ApiError::BadRequest {
        code: "missing_state",
        message: "OAuth callback missing `state` parameter".to_string(),
    })?;

    let pkce_verifier = db::get_and_delete_oauth_state(&state.pool, &oauth_state)
        .await?
        .ok_or(ApiError::Unauthorized {
            message: "Invalid or expired OAuth state".to_string(),
        })?;

    let cfg = AppConfig::from_env();
    let client_id = cfg.google_client_id.ok_or(ApiError::NotFound {
        resource: "oauth",
        id: "google_client_id_not_configured".to_string(),
    })?;
    let client_secret = cfg.google_client_secret.ok_or(ApiError::NotFound {
        resource: "oauth",
        id: "google_client_secret_not_configured".to_string(),
    })?;

    let http = reqwest::Client::new();

    // Exchange code for access_token
    let token_res = http
        .post(&cfg.oauth_google_token_url)
        .form(&[
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("code", code.as_str()),
            ("redirect_uri", cfg.oauth_redirect_url.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", pkce_verifier.as_str()),
        ])
        .send()
        .await
        .map_err(|e| {
            warn!("OAuth token exchange failed: {e}");
            ApiError::Internal { message: "token_exchange_failed".to_string() }
        })?;

    if !token_res.status().is_success() {
        warn!("OAuth token endpoint returned {}", token_res.status());
        return Err(ApiError::Internal {
            message: "token_exchange_failed".to_string(),
        });
    }

    let token: TokenResponse = token_res.json().await.map_err(|e| {
        warn!("OAuth token parse failed: {e}");
        ApiError::Internal { message: "token_parse_failed".to_string() }
    })?;

    // Fetch user info
    let userinfo: UserinfoResponse = http
        .get(&cfg.oauth_google_userinfo_url)
        .bearer_auth(&token.access_token)
        .send()
        .await
        .map_err(|e| {
            warn!("OAuth userinfo fetch failed: {e}");
            ApiError::Internal { message: "userinfo_fetch_failed".to_string() }
        })?
        .json()
        .await
        .map_err(|e| {
            warn!("OAuth userinfo parse failed: {e}");
            ApiError::Internal { message: "userinfo_parse_failed".to_string() }
        })?;

    // Upsert user: create if new, update display_name/email/refresh_token if existing.
    // INSERT OR IGNORE would silently skip updates for returning users.
    let display_name = userinfo
        .name
        .clone()
        .or_else(|| userinfo.email.clone())
        .unwrap_or_else(|| "Google User".to_string());

    let _ = sqlx::query(
        "INSERT INTO users \
         (user_id, display_name, email, auth_provider, auth_subject, google_refresh_token, created_at) \
         VALUES (?, ?, ?, 'google', ?, ?, ?) \
         ON CONFLICT(auth_provider, auth_subject) DO UPDATE SET \
           display_name = excluded.display_name, \
           email        = excluded.email, \
           google_refresh_token = COALESCE(excluded.google_refresh_token, google_refresh_token)",
    )
    .bind(db::new_user_id())
    .bind(&display_name)
    .bind(userinfo.email.as_deref())
    .bind(&userinfo.sub)
    .bind(token.refresh_token.as_deref())
    .bind(db::epoch_secs())
    .execute(&state.pool)
    .await;

    let user_id: String = sqlx::query_scalar(
        "SELECT user_id FROM users WHERE auth_provider = 'google' AND auth_subject = ?",
    )
    .bind(&userinfo.sub)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| ApiError::Internal {
        message: "user_lookup_failed".to_string(),
    })?;

    let session_token = db::generate_session_token();
    let session = db::create_user_session(
        &state.pool,
        &session_token,
        &user_id,
        &format!("web-{}", db::new_user_id()),
        db::SESSION_TTL_SECONDS,
    )
    .await?;

    if let Ok(Some(vault)) = db::get_vault_params(&state.pool).await {
        let email_json = match &userinfo.email {
            Some(e) => format!(r#""{}""#, e),
            None => "null".to_string(),
        };
        let _ = db::insert_audit_log(
            &state.pool,
            &vault.vault_id,
            "oauth_login",
            Some(&user_id),
            None,
            None,
            None,
            Some(&format!(r#"{{"provider":"google","email":{email_json}}}"#)),
        )
        .await;
    }

    Ok(Redirect::temporary(&format!(
        "/#oauth_token={}&expires_at={}",
        session_token, session.expires_at
    )))
}

// ── Routes ────────────────────────────────────────────────────────────

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/api/auth/google/start", get(get_google_start))
        .route("/api/auth/google/callback", get(get_google_callback))
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_pair_produces_valid_lengths() {
        let (verifier, challenge) = pkce_pair();
        // base64url of 96 bytes = 128 chars (no padding)
        assert_eq!(verifier.len(), 128);
        // base64url of 32-byte SHA-256 = 43 chars (no padding)
        assert_eq!(challenge.len(), 43);
        assert!(verifier.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert!(challenge.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn pkce_challenge_matches_verifier() {
        let (verifier, challenge) = pkce_pair();
        let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, expected);
    }

    #[test]
    fn pkce_pairs_are_unique() {
        let (v1, _) = pkce_pair();
        let (v2, _) = pkce_pair();
        assert_ne!(v1, v2);
    }
}
