use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::acl::{self, AuthorizedCaller, Role};
use crate::db::UserSession;

use super::ApiState;
use super::error::ApiError;

pub(super) struct ViewerCaller(pub AuthorizedCaller);
pub(super) struct MemberCaller(pub AuthorizedCaller);
pub(super) struct AdminCaller(pub AuthorizedCaller);
pub(super) struct SessionCaller(pub UserSession);

macro_rules! role_gate {
    ($ty:ident, $role:expr) => {
        impl FromRequestParts<ApiState> for $ty {
            type Rejection = ApiError;

            async fn from_request_parts(
                parts: &mut Parts,
                state: &ApiState,
            ) -> Result<Self, Self::Rejection> {
                acl::require_role(&state.pool, &parts.headers, $role)
                    .await
                    .map($ty)
            }
        }
    };
}

role_gate!(ViewerCaller, Role::Viewer);
role_gate!(MemberCaller, Role::Member);
role_gate!(AdminCaller, Role::Admin);

impl FromRequestParts<ApiState> for SessionCaller {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ApiState,
    ) -> Result<Self, Self::Rejection> {
        acl::require_session(&state.pool, &parts.headers)
            .await
            .map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_authorization_header_is_rejected_before_the_body_is_parsed() {
        let pool = crate::db::init_db("sqlite::memory:").await.expect("db");
        let headers = axum::http::HeaderMap::new();
        let result = acl::require_role(&pool, &headers, Role::Viewer).await;
        assert!(
            matches!(result, Err(ApiError::Unauthorized { .. })),
            "brak tokenu to 401, nie 403 ani 400"
        );
    }
}
