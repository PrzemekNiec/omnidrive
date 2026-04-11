use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug)]
pub enum ApiError {
    // 4xx
    BadRequest { code: &'static str, message: String },
    Unauthorized { message: String },
    Forbidden { message: String },
    NotFound { resource: &'static str, id: String },
    Conflict { message: String },
    Gone { message: String },
    Locked { message: String },
    // 5xx
    Internal { message: String },
    BadGateway { message: String },
    ServiceUnavailable { message: String },
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadRequest { code, message } => write!(f, "bad_request({code}): {message}"),
            Self::Unauthorized { message } => write!(f, "unauthorized: {message}"),
            Self::Forbidden { message } => write!(f, "forbidden: {message}"),
            Self::NotFound { resource, id } => write!(f, "not_found: {resource} '{id}'"),
            Self::Conflict { message } => write!(f, "conflict: {message}"),
            Self::Gone { message } => write!(f, "gone: {message}"),
            Self::Locked { message } => write!(f, "locked: {message}"),
            Self::Internal { message } => write!(f, "internal: {message}"),
            Self::BadGateway { message } => write!(f, "bad_gateway: {message}"),
            Self::ServiceUnavailable { message } => write!(f, "service_unavailable: {message}"),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::BadRequest { code, message } => (StatusCode::BAD_REQUEST, *code, message.clone()),
            Self::Unauthorized { message } => {
                (StatusCode::UNAUTHORIZED, "unauthorized", message.clone())
            }
            Self::Forbidden { message } => (StatusCode::FORBIDDEN, "forbidden", message.clone()),
            Self::NotFound { resource, id } => (
                StatusCode::NOT_FOUND,
                "not_found",
                format!("{resource} '{id}' not found"),
            ),
            Self::Conflict { message } => (StatusCode::CONFLICT, "conflict", message.clone()),
            Self::Gone { message } => (StatusCode::GONE, "gone", message.clone()),
            Self::Locked { message } => (StatusCode::LOCKED, "locked", message.clone()),
            Self::Internal { message } => {
                tracing::error!("api request failed: {message}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    message.clone(),
                )
            }
            Self::BadGateway { message } => {
                (StatusCode::BAD_GATEWAY, "bad_gateway", message.clone())
            }
            Self::ServiceUnavailable { message } => (
                StatusCode::SERVICE_UNAVAILABLE,
                "service_unavailable",
                message.clone(),
            ),
        };
        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        Self::Internal {
            message: err.to_string(),
        }
    }
}

impl From<std::io::Error> for ApiError {
    fn from(err: std::io::Error) -> Self {
        Self::Internal {
            message: err.to_string(),
        }
    }
}

impl From<Box<dyn std::error::Error>> for ApiError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        Self::Internal {
            message: err.to_string(),
        }
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for ApiError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self::Internal {
            message: err.to_string(),
        }
    }
}
