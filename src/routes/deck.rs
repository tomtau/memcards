use std::sync::Arc;

use askama::Template;
use axum::{
    Extension, Form,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use tracing::warn;

use crate::{
    errors::ApiError,
    models::{Deck, DeckNew},
    router::AppState,
    routes::handle_render,
    sdk::AuthUser,
    templates::{self},
};

fn check_user_id(user_id: Option<String>) -> Result<String, ApiError> {
    let user_id = user_id.ok_or(ApiError::UserNotFoundOrUnauthorized)?;
    if user_id.is_empty() {
        warn!("User ID is empty, returning unauthorized error");
        return Err(ApiError::UserNotFoundOrUnauthorized);
    }
    Ok(user_id)
}

pub async fn fetch_decks(
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = check_user_id(user_id)?;
    let decks = sqlx::query_as::<_, Deck>("SELECT * FROM deck WHERE user_id = $1")
        .bind(&user_id)
        .fetch_all(&state.db)
        .await?;

    let template = templates::Decks { decks };
    handle_render(template.render())
}

pub async fn create_deck(
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Form(form): Form<DeckNew>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = check_user_id(user_id)?;
    let deck = sqlx::query_as::<_, Deck>(
        "INSERT INTO deck (name, user_id) VALUES ($1, $2) RETURNING id, name, user_id",
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
    let user_id = check_user_id(user_id)?;
    sqlx::query("DELETE FROM deck WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(&state.db)
        .await?;

    Ok(StatusCode::OK)
}

pub async fn update_deck(
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Form(form): Form<DeckNew>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = check_user_id(user_id)?;
    let deck = sqlx::query_as::<_, Deck>(
        "UPDATE deck SET name = $1 WHERE id = $2 AND user_id = $3 RETURNING id, name, user_id",
    )
    .bind(form.name)
    .bind(id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    let template = templates::DeckNewTemplate { deck };
    handle_render(template.render())
}
