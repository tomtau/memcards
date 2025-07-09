use std::sync::Arc;

use askama::Template;
use axum::{
    Extension, Form,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use serde::Deserialize;
use tracing::{debug, error, info, warn};

use crate::{
    config::AppConfig,
    errors::ApiError,
    models::{Deck, DeckNew},
    router::AppState,
    sdk::{AuthUser, verify_signed_user_token},
    templates::{self, WebViewTemplate},
};

fn handle_render(res: askama::Result<String>) -> Result<Html<String>, ApiError> {
    match res {
        Ok(html) => Ok(Html(html)),
        Err(e) => {
            error!("Template rendering failed: {}", e);
            Err(ApiError::TemplateError(e))
        }
    }
}

pub async fn fetch_decks(
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = user_id.ok_or(ApiError::UserNotFoundOrUnauthorized)?;
    if user_id.is_empty() {
        warn!("User ID is empty, returning unauthorized error");
        return Err(ApiError::UserNotFoundOrUnauthorized);
    }
    let decks = sqlx::query_as::<_, Deck>("SELECT * FROM DECKS WHERE USER_ID = $1")
        .bind(&user_id)
        .fetch_all(&state.db)
        .await?;

    let template = templates::Decks { decks };
    handle_render(template.render())
}

pub async fn styles() -> Result<impl IntoResponse, ApiError> {
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/css")
        .body(include_str!("../templates/styles.css").to_owned())?;

    Ok(response)
}

pub async fn create_deck(
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Form(form): Form<DeckNew>,
) -> Result<impl IntoResponse, ApiError> {
    let deck = sqlx::query_as::<_, Deck>(
        "INSERT INTO DECKS (name, user_id) VALUES ($1, $2) RETURNING id, name, user_id",
    )
    .bind(form.name)
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    let template = templates::DeckNewTemplate { deck };
    handle_render(template.render())
}

pub async fn delete_deck(
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, ApiError> {
    sqlx::query("DELETE FROM DECKS WHERE ID = $1 AND USER_ID = $2")
        .bind(id)
        .bind(user_id)
        .execute(&state.db)
        .await?;

    Ok(StatusCode::OK)
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
