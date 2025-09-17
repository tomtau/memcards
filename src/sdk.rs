pub(crate) mod app_session;
pub(crate) mod auth;
mod event_manager;
pub(crate) mod events;
pub(crate) mod layout_manager;

use std::sync::Arc;

use crate::{
    config::AppConfig,
    router::AppState,
    sdk::app_session::{AppSession, UserId},
};
use anyhow::{Context, Result};
use axum::{Extension, Json, extract::State, http::StatusCode, response::IntoResponse};
use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::srs::extract_settings;
use tracing::{debug, error, info, warn};

#[derive(Deserialize, Debug, Clone)]
pub struct WebhookRequest {
    pub r#type: String,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    #[serde(rename = "userId")]
    pub user_id: Option<UserId>,
    #[serde(rename = "augmentOSWebsocketUrl")]
    pub augmentos_websocket_url: Option<String>,
    #[serde(rename = "mentraOSWebsocketUrl")]
    pub mentraos_websocket_url: Option<String>,
    #[allow(dead_code)]
    pub timestamp: Option<String>,
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolCall {
    pub tool_id: String,
    pub tool_parameters: serde_json::Value,
}

impl AppState {
    /// Called when a session is being stopped
    async fn on_stop(&self, session_id: &str, user_id: &UserId, reason: &str) -> Result<()> {
        info!(
            "🛑 Session {} stopped for user {}. Reason: {}",
            session_id, user_id, reason
        );
        // Default implementation - override in your implementation
        Ok(())
    }

    /// Called when a tool call is received
    async fn on_tool_call(&self, tool_call: &ToolCall) -> Result<Option<String>> {
        debug!("🔧 Tool call received: {}", tool_call.tool_id);
        debug!("🔧 Parameters: {:?}", tool_call.tool_parameters);
        // Default implementation returns None - override to provide responses
        Ok(None)
    }
}

pub async fn webhook_handler(
    State(state): State<Arc<AppState>>,
    Extension(config): Extension<AppConfig>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    debug!("Received payload: {payload:?}");
    let payload: WebhookRequest = match serde_json::from_value(payload.clone()) {
        Ok(req) => req,
        Err(e) => {
            error!("Failed to parse payload: {e}");
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"status": "error", "message": "Invalid payload"})),
            );
        }
    };
    match payload.r#type.as_str() {
        "session_request" => {
            // Handle session request (renamed from "session")
            if let (Some(session_id), Some(user_id), Some(ws_url)) = (
                payload.session_id.clone(),
                payload.user_id.clone(),
                payload
                    .augmentos_websocket_url
                    .clone()
                    .or(payload.mentraos_websocket_url),
            ) {
                match Url::parse(&ws_url).context("Invalid WebSocket URL") {
                    Ok(parsed_url) => match parsed_url.domain() {
                        Some(domain) => {
                            let mut expected = config.cloud_domain.rsplitn(3, '.');
                            let expected_top_level = expected.next().unwrap_or("");
                            let expected_second_level = expected.next().unwrap_or("");
                            let mut actual = domain.rsplitn(3, '.');
                            let actual_top_level = actual.next().unwrap_or("");
                            let actual_second_level = actual.next().unwrap_or("");
                            if (expected_top_level != actual_top_level
                                && expected_second_level != actual_second_level)
                                && (actual_top_level != "glass" && actual_second_level != "mentra")
                            {
                                error!(
                                    "WebSocket URL domain mismatch: expected {} but got {domain}",
                                    config.cloud_domain
                                );
                                return (
                                    StatusCode::BAD_REQUEST,
                                    Json(
                                        serde_json::json!({"status": "error", "message": "WebSocket URL domain mismatch"}),
                                    ),
                                );
                            }
                            info!("WebSocket URL is valid: {domain}");
                        }
                        None => {
                            error!("WebSocket URL has no valid domain");
                            return (
                                StatusCode::BAD_REQUEST,
                                Json(
                                    serde_json::json!({"status": "error", "message": "WebSocket URL has no valid domain"}),
                                ),
                            );
                        }
                    },
                    Err(e) => {
                        error!("Invalid WebSocket URL: {}", e);
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(
                                serde_json::json!({"status": "error", "message": format!("Invalid WebSocket URL: {}", e)}),
                            ),
                        );
                    }
                };
                let mut session = AppSession::new(
                    session_id.clone(),
                    user_id.clone(),
                    config.package_name.clone(),
                    config.api_key.clone(),
                    Some(ws_url),
                );

                // Attempt to connect the session
                match session.connect().await {
                    Ok(()) => {
                        info!("✅ Connected session {} for user {}", session_id, user_id);

                        // Call the session handler's on_session method
                        match state.on_session(&session, &session_id, &user_id).await {
                            Ok(()) => {
                                // Store the session after successful handling
                                state.active_sessions.insert(session_id.clone(), session);
                                info!(
                                    "✅ Session {} fully initialized for user {}",
                                    session_id, user_id
                                );
                                (
                                    StatusCode::OK,
                                    Json(serde_json::json!({"status": "success"})),
                                )
                            }
                            Err(e) => {
                                error!(
                                    "❌ Session handler failed for session {}: {}",
                                    session_id, e
                                );
                                session.disconnect();
                                (
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    Json(
                                        serde_json::json!({"status": "error", "message": format!("Session handling failed: {}", e)}),
                                    ),
                                )
                            }
                        }
                    }
                    Err(e) => {
                        error!("❌ Failed to connect session {}: {}", session_id, e);
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(
                                serde_json::json!({"status": "error", "message": format!("Failed to connect: {}", e)}),
                            ),
                        )
                    }
                }
            } else {
                debug!(
                    "[webhook] Missing required fields for session_request: session_id={:?}, user_id={:?}, augmentos_websocket_url={:?}",
                    payload.session_id, payload.user_id, payload.augmentos_websocket_url
                );
                (
                    StatusCode::BAD_REQUEST,
                    Json(
                        serde_json::json!({"status": "error", "message": "Missing required fields"}),
                    ),
                )
            }
        }
        "stop_request" => {
            if let (Some(session_id), Some(user_id), Some(reason)) = (
                payload.session_id.clone(),
                payload.user_id.clone(),
                payload.reason.clone(),
            ) {
                // Call the session handler's on_stop method first
                match state.on_stop(&session_id, &user_id, &reason).await {
                    Ok(()) => {
                        // Properly disconnect and remove the session
                        if let Some((_, mut session)) = state.active_sessions.remove(&session_id) {
                            session.disconnect();
                            info!(
                                "🛑 Stopped and disconnected session {} for user {}: {}",
                                session_id, user_id, reason
                            );
                        } else {
                            warn!("⚠️ Attempted to stop non-existent session {}", session_id);
                        }
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({"status": "success"})),
                        )
                    }
                    Err(e) => {
                        error!(
                            "❌ Session stop handler failed for session {}: {}",
                            session_id, e
                        );
                        // Still try to clean up the session even if handler failed
                        if let Some((_, mut session)) = state.active_sessions.remove(&session_id) {
                            session.disconnect();
                        }
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(
                                serde_json::json!({"status": "error", "message": format!("Stop handler failed: {}", e)}),
                            ),
                        )
                    }
                }
            } else {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"status": "error", "message": "Missing fields"})),
                )
            }
        }
        _ => {
            debug!("[webhook] Unknown webhook type: {}", payload.r#type);
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"status": "error", "message": "Unknown webhook type"})),
            )
        }
    }
}

pub(crate) async fn tool_handler(
    State(state): State<Arc<AppState>>,
    Json(tool_call): Json<ToolCall>,
) -> impl IntoResponse {
    info!(
        "🔧 Tool call: {} params: {:?}",
        tool_call.tool_id, tool_call.tool_parameters
    );

    // Call the session handler's tool call method
    match state.on_tool_call(&tool_call).await {
        Ok(response) => Json(serde_json::json!({
            "status": "success",
            "reply": response
        })),
        Err(e) => {
            error!("❌ Tool call handler failed: {}", e);
            Json(serde_json::json!({
                "status": "error",
                "message": format!("Tool call failed: {}", e)
            }))
        }
    }
}

pub(crate) async fn tool_get_handler() -> impl IntoResponse {
    Json(serde_json::json!({"status": "success", "reply": "Hello, world!"}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingsPayload {
    user_id_for_settings: UserId,
    settings: Vec<serde_json::Value>,
}
pub(crate) async fn settings_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SettingsPayload>,
) -> impl IntoResponse {
    let mut new_max_cards_per_session = None;
    let mut new_desired_retention = None;
    for setting in &payload.settings {
        extract_settings(
            &mut new_max_cards_per_session,
            &mut new_desired_retention,
            setting,
        );
    }
    let mut updated = 0;
    if new_desired_retention.is_some() || new_max_cards_per_session.is_some() {
        info!(
            "[settings] Settings updated for user {}: max_cards_per_session={:?}, desired_retention={:?}",
            payload.user_id_for_settings, new_max_cards_per_session, new_desired_retention
        );

        for session in state.active_sessions.iter() {
            if session.user_id == payload.user_id_for_settings {
                info!(
                    "[settings] Updating session {} for user {} with new settings",
                    session.session_id, payload.user_id_for_settings
                );
                if let Some(max_cards) = new_max_cards_per_session {
                    session
                        .user_settings
                        .set_max_cards_per_session(max_cards as u8);
                }
                if let Some(retention) = new_desired_retention {
                    session.user_settings.set_desired_retention(retention as u8);
                }
                updated += 1;
            }
        }
    } else {
        warn!(
            "[settings] No valid settings found in payload for user {}",
            payload.user_id_for_settings
        );
    }
    Json(serde_json::json!({"status": "success", "sessionsUpdated": updated}))
}

pub(crate) async fn health_handler(
    State(state): State<Arc<AppState>>,
    Extension(config): Extension<AppConfig>,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "app": config.package_name,
        "activeSessions": state.active_sessions.len()
    }))
}
