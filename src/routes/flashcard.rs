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
    models::{Deck, Flashcard, FlashcardNew, FlashcardUpdate, FlashcardWithReviews, Review},
    router::AppState,
    routes::{check_user_id, handle_render},
    sdk::AuthUser,
    templates::{FlashcardListTemplate, FlashcardTemplate, FlashcardsTemplate},
};

async fn get_deck_and_cards(
    state: Arc<AppState>,
    user_id: Option<String>,
    deck_id: i32,
) -> Result<(Deck, Vec<FlashcardWithReviews>), ApiError> {
    let user_id = check_user_id(user_id)?;

    // Get the deck info
    let deck = sqlx::query_as::<_, Deck>("SELECT * FROM deck WHERE id = $1 AND user_id = $2")
        .bind(deck_id)
        .bind(&user_id)
        .fetch_optional(&state.db)
        .await?;

    let deck = deck.ok_or(ApiError::UserNotFoundOrUnauthorized)?;

    // Get all flashcards with their reviews
    let rows = sqlx::query(
        r#"
        SELECT 
            f.id as flashcard_id,
            f.deck_id,
            f.front,
            f.back,
            r.id as review_id,
            r.reviewed,
            r.scheduled,
            r.rating,
            r.stability,
            r.difficulty,
            r.flashcard_id as review_flashcard_id
        FROM flashcard f
        LEFT JOIN review r ON f.id = r.flashcard_id
        WHERE f.deck_id = $1
        ORDER BY f.id, r.reviewed DESC
        "#,
    )
    .bind(deck_id)
    .fetch_all(&state.db)
    .await?;

    // Group reviews by flashcard
    let mut flashcards_map: std::collections::HashMap<i32, FlashcardWithReviews> =
        std::collections::HashMap::new();

    for row in rows {
        let flashcard_id: i32 = row.get("flashcard_id");

        let flashcard =
            flashcards_map
                .entry(flashcard_id)
                .or_insert_with(|| FlashcardWithReviews {
                    id: flashcard_id,
                    deck_id: row.get("deck_id"),
                    front: row.get("front"),
                    back: row.get("back"),
                    reviews: Vec::new(),
                });

        // Add review if it exists
        if let Some(review_id) = row.get::<Option<i32>, _>("review_id") {
            let review = Review {
                id: review_id,
                reviewed: row.get("reviewed"),
                scheduled: row.get("scheduled"),
                rating: row.get("rating"),
                stability: row.get("stability"),
                difficulty: row.get("difficulty"),
                flashcard_id: row.get("review_flashcard_id"),
            };
            flashcard.reviews.push(review);
        }
    }

    let flashcards: Vec<FlashcardWithReviews> = flashcards_map.into_values().collect();

    Ok((deck, flashcards))
}

// List flashcards page for a deck
pub async fn list_flashcards_page(
    Extension(AuthUser(user_id)): Extension<AuthUser>,
    State(state): State<Arc<AppState>>,
    Path(deck_id): Path<i32>,
) -> Result<impl IntoResponse, ApiError> {
    let (_deck, flashcards) = get_deck_and_cards(state, user_id, deck_id).await?;
    let template = FlashcardListTemplate { flashcards };
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
        .fetch_optional(&state.db)
        .await?;

    if deck_exists.is_none() {
        return Err(ApiError::UserNotFoundOrUnauthorized);
    }

    let flashcard = sqlx::query_as::<_, Flashcard>(
        "INSERT INTO flashcard (deck_id, front, back) VALUES ($1, $2, $3) RETURNING id, deck_id, front, back",
    )
    .bind(form.deck_id)
    .bind(form.front)
    .bind(form.back)
    .fetch_one(&state.db)
    .await?;

    // Convert to FlashcardWithReviews for template
    let flashcard_with_reviews = FlashcardWithReviews {
        id: flashcard.id,
        deck_id: flashcard.deck_id,
        front: flashcard.front,
        back: flashcard.back,
        reviews: Vec::new(),
    };

    let template = FlashcardTemplate {
        flashcard: flashcard_with_reviews,
    };
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
        RETURNING id, deck_id, front, back
        "#,
    )
    .bind(form.front)
    .bind(form.back)
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    match flashcard {
        Some(card) => {
            let flashcard_with_reviews = FlashcardWithReviews {
                id: card.id,
                deck_id: card.deck_id,
                front: card.front,
                back: card.back,
                reviews: Vec::new(),
            };

            let template = FlashcardTemplate {
                flashcard: flashcard_with_reviews,
            };
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
    .execute(&state.db)
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
    let rows = sqlx::query(
        r#"
        SELECT 
            f.id as flashcard_id,
            f.deck_id,
            f.front,
            f.back,
            r.id as review_id,
            r.reviewed,
            r.scheduled,
            r.rating,
            r.stability,
            r.difficulty,
            r.flashcard_id as review_flashcard_id
        FROM flashcard f
        LEFT JOIN review r ON f.id = r.flashcard_id
        WHERE f.id = $1 AND f.deck_id IN (
            SELECT id FROM deck WHERE user_id = $2
        )
        ORDER BY r.reviewed DESC
        "#,
    )
    .bind(id)
    .bind(&user_id)
    .fetch_all(&state.db)
    .await?;

    if rows.is_empty() {
        return Err(ApiError::UserNotFoundOrUnauthorized);
    }

    let first_row = &rows[0];
    let mut flashcard = FlashcardWithReviews {
        id: first_row.get("flashcard_id"),
        deck_id: first_row.get("deck_id"),
        front: first_row.get("front"),
        back: first_row.get("back"),
        reviews: Vec::new(),
    };

    for row in rows {
        if let Some(review_id) = row.get::<Option<i32>, _>("review_id") {
            let review = Review {
                id: review_id,
                reviewed: row.get("reviewed"),
                scheduled: row.get("scheduled"),
                rating: row.get("rating"),
                stability: row.get("stability"),
                difficulty: row.get("difficulty"),
                flashcard_id: row.get("review_flashcard_id"),
            };
            flashcard.reviews.push(review);
        }
    }

    let flashcard_with_reviews = FlashcardWithReviews {
        id: flashcard.id,
        deck_id: flashcard.deck_id,
        front: flashcard.front,
        back: flashcard.back,
        reviews: Vec::new(),
    };

    let template = FlashcardTemplate {
        flashcard: flashcard_with_reviews,
    };
    handle_render(template.render())
}
