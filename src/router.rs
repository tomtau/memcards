use std::sync::Arc;

use axum::{
    Extension, Router, middleware,
    routing::{delete, get, post},
};
use dashmap::DashMap;
use sqlx::PgPool;

use crate::{
    config::AppConfig,
    routes,
    sdk::{
        app_session::AppSession, auth_middleware, health_handler, settings_handler,
        tool_get_handler, tool_handler, webhook_handler,
    },
};

pub struct AppState {
    pub db: Arc<PgPool>,
    pub active_sessions: DashMap<String, AppSession>,
}

pub fn init_router(db: PgPool, config: AppConfig) -> Router {
    let state = Arc::new(AppState {
        db: Arc::new(db),
        active_sessions: DashMap::new(),
    });
    // Create webhook routes that bypass authentication
    let webhook_routes = Router::new()
        .route("/webhook", post(webhook_handler))
        .with_state(state.clone());
    // Create authenticated routes
    let auth_routes = Router::new()
        // TODO: check if tool, settings, and health routes are needed
        .route("/tool", post(tool_handler).get(tool_get_handler))
        .route("/settings", post(settings_handler))
        .route("/health", get(health_handler))
        .route("/webview", get(routes::webview_handler))
        .route("/styles.css", get(routes::styles))
        .route("/decks", get(routes::fetch_decks).post(routes::create_deck))
        .route(
            "/decks/{id}",
            delete(routes::delete_deck).put(routes::update_deck),
        )
        .route(
            "/decks/{id}/import",
            get(routes::show_import_form).post(routes::import_deck),
        )
        .route(
            "/decks/{deck_id}/flashcards",
            get(routes::view_flashcards_page).post(routes::create_flashcard),
        )
        .route(
            "/decks/{deck_id}/flashcards/list",
            get(routes::list_flashcards_page),
        )
        .route(
            "/flashcards/{id}",
            get(routes::get_flashcard)
                .put(routes::update_flashcard)
                .delete(routes::delete_flashcard),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .with_state(state);
    Router::new()
        .merge(webhook_routes)
        .merge(auth_routes)
        .layer(Extension(config.clone()))
}
