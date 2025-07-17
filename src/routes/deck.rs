use std::sync::Arc;

use askama::Template;
use axum::{
    Extension, Form,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use sqlx::Row;

use crate::{
    errors::ApiError,
    models::{Deck, DeckNew},
    router::AppState,
    routes::{check_user_id, handle_render},
    sdk::AuthUser,
    templates::{self},
};

pub async fn fetch_decks(
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = check_user_id(user_id)?;
    let decks = sqlx::query_as::<_, Deck>("SELECT * FROM deck WHERE user_id = $1")
        .bind(&user_id)
        .fetch_all(&*state.db)
        .await?;

    // Calculate statistics for all flashcards across all decks
    let stats_query = r#"
        SELECT 
            COUNT(CASE WHEN last_rating IS NULL THEN 1 END) as new_count,
            COUNT(CASE WHEN last_scheduled IS NOT NULL AND last_scheduled <= NOW() THEN 1 END) as for_review_count,
            COUNT(CASE WHEN last_scheduled IS NOT NULL AND last_scheduled > NOW() THEN 1 END) as learning_count
        FROM flashcard f
        INNER JOIN deck d ON f.deck_id = d.id
        WHERE d.user_id = $1
    "#;
    
    let stats_row = sqlx::query(stats_query)
        .bind(&user_id)
        .fetch_one(&*state.db)
        .await?;
    
    let stats = crate::models::FlashcardStats {
        new_count: stats_row.get("new_count"),
        for_review_count: stats_row.get("for_review_count"),
        learning_count: stats_row.get("learning_count"),
    };

    let template = templates::Decks { decks, stats };
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
    .fetch_one(&*state.db)
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
        .execute(&*state.db)
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
    .fetch_one(&*state.db)
    .await?;

    let template = templates::DeckNewTemplate { deck };
    handle_render(template.render())
}
