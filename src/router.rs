use std::{collections::HashMap, sync::Arc};

use axum::{
    Extension, Router, middleware,
    routing::{delete, get, post},
};
use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::{
    config::AppConfig,
    routes,
    sdk::{
        SessionHandler, app_session::AppSession, auth_middleware, health_handler, settings_handler,
        tool_get_handler, tool_handler, webhook_handler,
    },
};

pub struct AppState {
    pub db: PgPool,
    pub active_sessions: Mutex<HashMap<String, AppSession>>,
    pub session_handler: SessionHandler,
}

pub fn init_router(db: PgPool, config: AppConfig) -> Router {
    let state = Arc::new(AppState {
        db,
        active_sessions: Mutex::new(HashMap::new()),
        session_handler: SessionHandler,
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
        .route("/decks/:id", delete(routes::delete_deck))
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
