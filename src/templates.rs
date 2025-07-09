use askama::Template;

use crate::models;

#[derive(Template)]
#[template(path = "webview.html")]
pub struct WebViewTemplate {
    pub is_authenticated: bool,
    pub auth_token: String,
}

#[derive(Template)]
#[template(path = "decks.html")]
pub struct Decks {
    pub decks: Vec<models::Deck>,
}

#[derive(Template)]
#[template(path = "deck.html")]
pub struct DeckNewTemplate {
    pub deck: models::Deck,
}
