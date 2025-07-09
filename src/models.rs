use serde::{Deserialize, Serialize};

#[derive(sqlx::FromRow, Serialize, Deserialize)]
pub struct Deck {
    pub id: i32,
    pub name: String,
    pub user_id: String,
}

#[derive(sqlx::FromRow, Serialize, Deserialize)]
pub struct DeckNew {
    pub name: String,
}
