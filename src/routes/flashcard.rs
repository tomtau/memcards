use std::sync::Arc;

use askama::Template;
use axum::{
    Extension, Form,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;

use crate::{
    errors::ApiError,
    models::{Deck, Flashcard, FlashcardNew, FlashcardUpdate},
    router::AppState,
    routes::{check_user_id, handle_render},
    sdk::{app_session::UserId, auth::AuthUser},
    templates::{FlashcardListTemplate, FlashcardTemplate, FlashcardsTemplate},
};

#[derive(Deserialize)]
pub struct PaginationQuery {
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

async fn get_deck_and_cards(
    state: Arc<AppState>,
    user_id: Option<UserId>,
    deck_id: i32,
) -> Result<(Deck, Vec<Flashcard>), ApiError> {
    let user_id = check_user_id(user_id)?;

    // Get the deck info
    let deck = sqlx::query_as::<_, Deck>("SELECT * FROM deck WHERE id = $1 AND user_id = $2")
        .bind(deck_id)
        .bind(&user_id)
        .fetch_optional(&*state.db)
        .await?;

    let deck = deck.ok_or(ApiError::UserNotFoundOrUnauthorized)?;

    // Get all flashcards for the deck
    let flashcards = sqlx::query_as::<_, Flashcard>(
        "SELECT * FROM flashcard WHERE deck_id = $1 ORDER BY last_reviewed DESC, id",
    )
    .bind(deck_id)
    .fetch_all(&*state.db)
    .await?;
    Ok((deck, flashcards))
}

async fn get_deck_and_cards_paginated(
    state: Arc<AppState>,
    user_id: Option<UserId>,
    deck_id: i32,
    page: u32,
    limit: u32,
) -> Result<(Deck, Vec<Flashcard>, bool), ApiError> {
    let user_id = check_user_id(user_id)?;

    // Get the deck info
    let deck = sqlx::query_as::<_, Deck>("SELECT * FROM deck WHERE id = $1 AND user_id = $2")
        .bind(deck_id)
        .bind(&user_id)
        .fetch_optional(&*state.db)
        .await?;

    let deck = deck.ok_or(ApiError::UserNotFoundOrUnauthorized)?;

    let offset = page * limit;

    // Get flashcards for the deck with pagination (get one extra to check if there are more)
    let flashcards = sqlx::query_as::<_, Flashcard>(
        "SELECT * FROM flashcard WHERE deck_id = $1 ORDER BY last_reviewed DESC, id LIMIT $2 OFFSET $3",
    )
    .bind(deck_id)
    .bind((limit + 1) as i64) // Get one extra to check if there are more
    .bind(offset as i64)
    .fetch_all(&*state.db)
    .await?;

    // Check if there are more flashcards
    let has_more = flashcards.len() > limit as usize;
    let mut result_flashcards = flashcards;
    if has_more {
        result_flashcards.pop(); // Remove the extra one
    }

    Ok((deck, result_flashcards, has_more))
}

// List flashcards page for a deck (with pagination support)
pub async fn list_flashcards_page(
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(deck_id): Path<i32>,
    Query(pagination): Query<PaginationQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let page = pagination.page.unwrap_or(0);
    let limit = pagination.limit.unwrap_or(20); // Default to 20 flashcards per page

    let (_deck, flashcards, has_more) =
        get_deck_and_cards_paginated(state, user_id, deck_id, page, limit).await?;

    // Create the template with pagination info
    let template = FlashcardListTemplate {
        flashcards,
        deck_id,
        page,
        has_more,
    };
    handle_render(template.render())
}

// View flashcards page for a deck
pub async fn view_flashcards_page(
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(deck_id): Path<i32>,
) -> Result<impl IntoResponse, ApiError> {
    let (deck, _flashcards) = get_deck_and_cards(state, user_id, deck_id).await?;
    let template = FlashcardsTemplate {
        is_authenticated: true,
        deck,
    };
    handle_render(template.render())
}

// Create a new flashcard
pub async fn create_flashcard(
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Form(form): Form<FlashcardNew>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = check_user_id(user_id)?;

    // Verify the user owns the deck
    let deck_exists = sqlx::query("SELECT 1 FROM deck WHERE id = $1 AND user_id = $2")
        .bind(form.deck_id)
        .bind(&user_id)
        .fetch_optional(&*state.db)
        .await?;

    if deck_exists.is_none() {
        return Err(ApiError::UserNotFoundOrUnauthorized);
    }

    let flashcard = sqlx::query_as::<_, Flashcard>(
        "INSERT INTO flashcard (deck_id, front, back) VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(form.deck_id)
    .bind(form.front)
    .bind(form.back)
    .fetch_one(&*state.db)
    .await?;

    let template = FlashcardTemplate { flashcard };
    handle_render(template.render())
}

// Update a flashcard
pub async fn update_flashcard(
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Form(form): Form<FlashcardUpdate>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = check_user_id(user_id)?;

    // Verify the user owns the flashcard through the deck
    let flashcard = sqlx::query_as::<_, Flashcard>(
        r#"
        UPDATE flashcard 
        SET front = $1, back = $2 
        WHERE id = $3 AND deck_id IN (
            SELECT id FROM deck WHERE user_id = $4
        )
        RETURNING *
        "#,
    )
    .bind(form.front)
    .bind(form.back)
    .bind(id)
    .bind(user_id)
    .fetch_optional(&*state.db)
    .await?;

    match flashcard {
        Some(flashcard) => {
            let template = FlashcardTemplate { flashcard };
            handle_render(template.render())
        }
        None => Err(ApiError::UserNotFoundOrUnauthorized),
    }
}

// Delete a flashcard
pub async fn delete_flashcard(
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = check_user_id(user_id)?;

    let result = sqlx::query(
        r#"
        DELETE FROM flashcard 
        WHERE id = $1 AND deck_id IN (
            SELECT id FROM deck WHERE user_id = $2
        )
        "#,
    )
    .bind(id)
    .bind(user_id)
    .execute(&*state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(ApiError::UserNotFoundOrUnauthorized);
    }

    Ok(StatusCode::OK)
}

// Get a single flashcard with reviews
pub async fn get_flashcard(
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = check_user_id(user_id)?;

    // Get flashcard with reviews
    let flashcard = sqlx::query_as::<_, Flashcard>(
        r#"
        SELECT 
            *
        FROM flashcard f
        WHERE f.id = $1 AND f.deck_id IN (
            SELECT id FROM deck WHERE user_id = $2
        )
        ORDER BY r.reviewed DESC, LIMIT 1
        "#,
    )
    .bind(id)
    .bind(&user_id)
    .fetch_one(&*state.db)
    .await?;

    let template = FlashcardTemplate { flashcard };
    handle_render(template.render())
}
