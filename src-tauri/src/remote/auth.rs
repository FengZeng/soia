use super::state::{is_active_session, is_enabled, RemoteControlState};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorResponse {
    error: String,
}

pub(super) struct RemoteError {
    status: StatusCode,
    message: String,
}

impl RemoteError {
    pub(super) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub(super) fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }
}

impl IntoResponse for RemoteError {
    fn into_response(self) -> Response {
        with_cors((
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        ))
    }
}

pub(super) fn authorize_headers(
    state: &RemoteControlState,
    headers: &HeaderMap,
) -> Result<(), RemoteError> {
    authorize(state, headers, None)
}

pub(super) fn authorize(
    state: &RemoteControlState,
    headers: &HeaderMap,
    query_token: Option<&str>,
) -> Result<(), RemoteError> {
    if !is_enabled(state) {
        return Err(RemoteError::unauthorized("remote control is disabled"));
    }
    let session = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(remote_session_from_cookie);
    if session.is_some_and(|session| is_active_session(state, session)) {
        return Ok(());
    }
    let Some(expected_token) = state.token.as_deref() else {
        return Ok(());
    };

    let provided_token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .or_else(|| {
            headers
                .get("x-soia-remote-token")
                .and_then(|value| value.to_str().ok())
        });

    if provided_token == Some(expected_token) || query_token == Some(expected_token) {
        Ok(())
    } else {
        Err(RemoteError::unauthorized(
            "missing or invalid remote control token",
        ))
    }
}

pub(super) fn remote_session_from_cookie(cookie: &str) -> Option<&str> {
    cookie
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("soia_remote_session="))
}

pub(super) fn with_cors(response: impl IntoResponse) -> Response {
    let mut response = response.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Authorization, Content-Type, X-Soia-Remote-Token"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("86400"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::remote_session_from_cookie;

    #[test]
    fn extracts_remote_session_from_cookie_list() {
        assert_eq!(
            remote_session_from_cookie("theme=dark; soia_remote_session=session-42; other=value"),
            Some("session-42")
        );
        assert_eq!(remote_session_from_cookie("theme=dark"), None);
    }
}
