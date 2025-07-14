use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(sqlx::FromRow, Serialize, Deserialize, Debug, Clone)]
pub struct Deck {
    pub id: i32,
    pub name: String,
    pub user_id: String,
}

#[derive(sqlx::FromRow, Serialize, Deserialize, Debug, Clone)]
pub struct DeckNew {
    pub name: String,
}

#[derive(sqlx::FromRow, Serialize, Deserialize, Debug, Clone)]
pub struct DeckRename {
    pub id: i32,
    pub name: String,
}

#[derive(sqlx::FromRow, Serialize, Deserialize, Debug, Clone)]
pub struct Flashcard {
    pub id: i32,
    pub deck_id: i32,
    pub front: String,
    pub back: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FlashcardNew {
    pub deck_id: i32,
    pub front: String,
    pub back: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FlashcardUpdate {
    pub front: String,
    pub back: String,
}

#[derive(sqlx::FromRow, Serialize, Deserialize, Debug, Clone)]
pub struct Review {
    pub id: i32,
    pub reviewed: DateTime<Utc>,
    pub scheduled: DateTime<Utc>,
    pub rating: String,
    pub stability: f32,
    pub difficulty: f32,
    pub flashcard_id: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FlashcardWithReviews {
    pub id: i32,
    pub deck_id: i32,
    pub front: String,
    pub back: String,
    pub reviews: Vec<Review>,
}
