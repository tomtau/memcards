mod deck;
mod flashcard;

pub use deck::*;
pub use flashcard::*;

use askama::Template;
use axum::{
    Extension,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use tracing::{error, warn};

use crate::{
    errors::ApiError,
    sdk::{app_session::UserId, auth::AuthUser},
    templates::WebViewTemplate,
};

fn check_user_id(user_id: Option<UserId>) -> Result<String, ApiError> {
    let user_id = user_id.ok_or(ApiError::UserNotFoundOrUnauthorized)?;
    if user_id.0.is_empty() {
        // TODO: can be put in a deserializer and TryFrom impl
        warn!("User ID is empty, returning unauthorized error");
        return Err(ApiError::UserNotFoundOrUnauthorized);
    }
    Ok(user_id.0)
}

fn handle_render(res: askama::Result<String>) -> Result<Html<String>, ApiError> {
    match res {
        Ok(html) => Ok(Html(html)),
        Err(e) => {
            error!("Template rendering failed: {}", e);
            Err(ApiError::TemplateError(e))
        }
    }
}

pub async fn styles() -> Result<impl IntoResponse, ApiError> {
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/css")
        .body(include_str!("../templates/styles.css").to_owned())?;

    Ok(response)
}

pub async fn webview_handler(
    Extension(AuthUser(user_id)): Extension<AuthUser>,
) -> impl IntoResponse {
    let template = WebViewTemplate {
        is_authenticated: user_id.is_some_and(|x| !x.0.is_empty()),
    };

    handle_render(template.render())
}
