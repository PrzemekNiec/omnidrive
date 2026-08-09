use axum::http::HeaderMap;

use super::error::ApiError;

const LOCAL_INTENT_HEADER: &str = "x-omnidrive-local";

/// Wymusza preflight CORS: przeglądarka nie wyśle tego nagłówka z obcego origin
/// bez zgody serwera, więc żądanie drive-by nie dojdzie do handlera.
pub(super) fn require_local_intent(headers: &HeaderMap) -> Result<(), ApiError> {
    let present = headers
        .get(LOCAL_INTENT_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim() == "1");

    if present {
        Ok(())
    } else {
        Err(ApiError::Forbidden {
            message: "missing local intent header".to_string(),
        })
    }
}
