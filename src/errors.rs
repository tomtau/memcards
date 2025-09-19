//! Error handling for the API
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

pub enum ApiError {
    SQLError(sqlx::Error),
    HTTPError(axum::http::Error),
    TemplateError(askama::Error),
    UserNotFoundOrUnauthorized,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::SQLError(e) => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("SQL error: {e}")).into_response()
            }
            Self::HTTPError(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("HTTP error: {e}"),
            )
                .into_response(),
            Self::TemplateError(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Template error: {e}"),
            )
                .into_response(),
            Self::UserNotFoundOrUnauthorized => (
                StatusCode::UNAUTHORIZED,
                "User not found or unauthorized".to_string(),
            )
                .into_response(),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        Self::SQLError(e)
    }
}

impl From<axum::http::Error> for ApiError {
    fn from(e: axum::http::Error) -> Self {
        Self::HTTPError(e)
    }
}

impl From<askama::Error> for ApiError {
    fn from(e: askama::Error) -> Self {
        Self::TemplateError(e)
    }
}
