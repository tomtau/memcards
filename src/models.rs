//! Models for the database
use std::{fmt::Display, str::FromStr};

use chrono::NaiveDateTime;
use fsrs::MemoryState;
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

#[derive(sqlx::FromRow, Serialize, Deserialize, Debug, Clone, Default)]
pub struct Flashcard {
    pub id: i32,
    pub deck_id: i32,
    pub front: String,
    pub back: String,
    pub last_rating: Option<CardRating>,
    pub last_reviewed: Option<NaiveDateTime>,
    pub last_scheduled: Option<NaiveDateTime>,
    pub last_stability: Option<f32>,
    pub last_difficulty: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FlashcardNew {
    pub deck_id: i32,
    pub front: String,
    pub back: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FlashcardImport {
    pub anki_text: String,
    pub front_idx: usize,
    pub back_idx: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FlashcardUpdate {
    pub front: String,
    pub back: String,
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, sqlx::Type, Deserialize, Serialize)]
#[sqlx(type_name = "card_rating", rename_all = "lowercase")]
pub enum CardRating {
    Easy,
    Good,
    Difficult,
    Again,
}

impl Display for CardRating {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CardRating::Easy => write!(f, "easy"),
            CardRating::Good => write!(f, "good"),
            CardRating::Difficult => write!(f, "difficult"),
            CardRating::Again => write!(f, "again"),
        }
    }
}

impl FromStr for CardRating {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains("easy") {
            Ok(CardRating::Easy)
        } else if s.contains("good") {
            Ok(CardRating::Good)
        } else if s.contains("difficult") {
            Ok(CardRating::Difficult)
        } else if s.contains("again") {
            Ok(CardRating::Again)
        } else {
            anyhow::bail!("Invalid card rating: {s}");
        }
    }
}

impl TryFrom<&Flashcard> for MemoryState {
    type Error = anyhow::Error;
    fn try_from(value: &Flashcard) -> Result<Self, Self::Error> {
        match (value.last_stability, value.last_difficulty) {
            (Some(stability), Some(difficulty)) => Ok(MemoryState {
                stability,
                difficulty,
            }),
            _ => Err(anyhow::anyhow!(
                "Flashcard does not have enough data to create MemoryState"
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FlashcardReviewNew {
    pub reviewed: NaiveDateTime,
    pub scheduled: NaiveDateTime,
    pub rating: CardRating,
    pub stability: f32,
    pub difficulty: f32,
    pub flashcard_id: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FlashcardStats {
    pub new_count: i64,
    pub for_review_count: i64,
    pub learning_count: i64,
}
