mod deck;

pub use deck::*;

use askama::Template;
use axum::{
    Extension,
    extract::Query,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use serde::Deserialize;
use tracing::{debug, error, info, warn};

use crate::{
    config::AppConfig, errors::ApiError, sdk::verify_signed_user_token, templates::WebViewTemplate,
};

pub(self) fn handle_render(res: askama::Result<String>) -> Result<Html<String>, ApiError> {
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

#[derive(Deserialize)]
pub struct WebViewQuery {
    pub aos_signed_user_token: Option<String>,
}

pub async fn webview_handler(
    Query(query): Query<WebViewQuery>,
    Extension(config): Extension<AppConfig>,
) -> impl IntoResponse {
    let (user_id, is_authenticated, auth_token) = if let Some(token) = query.aos_signed_user_token {
        match verify_signed_user_token(&token, &config.user_token_public_key) {
            Ok(uid) => {
                info!("User ID verified from JWT token: {}", uid);
                (uid, true, token)
            }
            Err(e) => {
                warn!("JWT token verification failed: {}", e);
                ("".to_string(), false, "".to_string())
            }
        }
    } else {
        debug!("No JWT token provided");
        ("".to_string(), false, "".to_string())
    };
    debug!(
        "User ID: {}, Authenticated: {}, Token: {}",
        user_id, is_authenticated, auth_token
    );

    let template = WebViewTemplate {
        is_authenticated,
        auth_token,
    };

    handle_render(template.render())
}
