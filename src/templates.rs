use crate::models;
use askama::Template;

#[derive(Template)]
#[template(path = "webview.html")]
pub struct WebViewTemplate {
    pub is_authenticated: bool,
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

#[derive(Template)]
#[template(path = "flashcards.html")]
pub struct FlashcardsTemplate {
    pub is_authenticated: bool,
    pub deck: models::Deck,
}

#[derive(Template)]
#[template(path = "flashcard.html")]
pub struct FlashcardTemplate {
    pub flashcard: models::Flashcard,
}

#[derive(Template)]
#[template(path = "flashcard_list.html")]
pub struct FlashcardListTemplate {
    pub flashcards: Vec<models::Flashcard>,
}
