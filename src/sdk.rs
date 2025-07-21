pub(crate) mod app_session;
mod event_manager;
pub(crate) mod events;
pub(crate) mod layout_manager;

use std::{collections::HashSet, sync::Arc, time::Duration};

use crate::{config::AppConfig, router::AppState, sdk::app_session::AppSession};
use anyhow::{Context, Result, bail};
use axum::{
    Extension, Json,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_extra::extract::{SignedCookieJar, cookie};
use jsonwebtoken::{Algorithm, DecodingKey, TokenData, Validation, decode};
use reqwest::{Client, Url};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::srs::extract_settings;
use tracing::{debug, error, info, warn};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WebhookRequest {
    pub r#type: String,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    #[serde(rename = "userId")]
    pub user_id: Option<String>,
    #[serde(rename = "augmentOSWebsocketUrl")]
    pub augmentos_websocket_url: Option<String>,
    #[serde(rename = "mentraOSWebsocketUrl")]
    pub mentraos_websocket_url: Option<String>,
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
    async fn on_stop(&self, session_id: &str, user_id: &str, reason: &str) -> Result<()> {
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

pub(crate) fn verify_signed_user_token(token: &str, public_key_pem: &str) -> Result<String> {
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_required_spec_claims(&["iss", "exp", "iat"]);
    validation.iss = Some(HashSet::from_iter([
        "https://prod.augmentos.cloud".to_string()
    ]));
    let key = DecodingKey::from_rsa_pem(public_key_pem.as_bytes()).context("Invalid public key")?;
    let token_data: TokenData<serde_json::Value> =
        decode(token, &key, &validation).context("JWT error")?;
    token_data
        .claims
        .get("sub")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("No 'sub' claim in JWT"))
}

#[derive(Clone, Debug)]
pub struct AuthUser(pub Option<String>);

fn get_query_param(query: Option<&str>, key: &str) -> Option<String> {
    query.and_then(|q| {
        q.split('&').find_map(|kv| {
            let mut split = kv.splitn(2, '=');
            match (split.next(), split.next()) {
                (Some(k), Some(v)) if k == key => Some(v.to_string()),
                _ => None,
            }
        })
    })
}

pub async fn auth_middleware(
    State(_state): State<Arc<AppState>>,
    Extension(config): Extension<AppConfig>,
    mut req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    debug!("Processing request: {} {}", req.method(), req.uri());
    let mut user_id: Option<String> = None;
    let headers = req.headers();
    let mut cookies = SignedCookieJar::from_headers(headers, config.cookie_secret.clone());

    debug!("Headers: {:?}", headers);
    debug!("Query string: {:?}", req.uri().query());

    // --- 1. JWT Signed User Token (query param) ---
    if let Some(signed_user_token) = get_query_param(req.uri().query(), "aos_signed_user_token") {
        match verify_signed_user_token(&signed_user_token, &config.user_token_public_key) {
            Ok(uid) => {
                user_id = Some(uid.clone());
                cookies = add_signed_cookie(cookies, &uid);
                info!("User ID verified from signed user token: {}", uid);
            }
            Err(e) => {
                warn!("Signed user token invalid: {}", e);
            }
        }
    }
    // --- 2. JWT Signed User Token (Authorization header) ---
    else if let Some(auth_header) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
    {
        // Try to verify as JWT token first
        match verify_signed_user_token(auth_header, &config.user_token_public_key) {
            Ok(uid) => {
                user_id = Some(uid.clone());
                cookies = add_signed_cookie(cookies, &uid);
                info!(
                    "User ID verified from JWT token in Authorization header: {}",
                    uid
                );
            }
            Err(e) => {
                debug!("Authorization header JWT verification failed: {}", e);
                // If JWT verification fails, try as frontend token
                match verify_frontend_token(auth_header, &config.api_key) {
                    Some(uid) => {
                        user_id = Some(uid.clone());
                        cookies = add_signed_cookie(cookies, &uid);
                        info!(
                            "User ID verified from frontend token in Authorization header: {}",
                            uid
                        );
                    }
                    None => {
                        warn!(
                            "Authorization header token invalid (tried both JWT and frontend token)"
                        );
                    }
                }
            }
        }
    }
    // --- 3. Temp Token (query param) ---
    else if let Some(temp_token) = get_query_param(req.uri().query(), "aos_temp_token") {
        match exchange_token_with_cloud(
            &config.cloud_api_url,
            &temp_token,
            &config.api_key,
            &config.package_name,
        )
        .await
        {
            Ok(uid) => {
                user_id = Some(uid.clone());
                cookies = add_signed_cookie(cookies, &uid);
                info!("User ID verified from temporary token: {}", uid);
            }
            Err(e) => {
                warn!("Temp token exchange failed: {}", e);
            }
        }
    }
    // --- 4. Frontend Token (query param only, since header is handled above) ---
    else if let Some(frontend_token) = get_query_param(req.uri().query(), "aos_frontend_token") {
        match verify_frontend_token(&frontend_token, &config.api_key) {
            Some(uid) => {
                user_id = Some(uid.clone());
                cookies = add_signed_cookie(cookies, &uid);
                info!("User ID verified from frontend user token: {}", uid);
            }
            None => {
                warn!("Frontend token invalid");
            }
        }
    }
    // --- 5. Session Cookie ---
    else if let Some(cookie) = cookies.get("aos_session") {
        info!("Session cookie found: {}", cookie.value());
        user_id = Some(cookie.value().to_string());
    }

    debug!("Final user_id: {:?}", user_id);
    req.extensions_mut().insert(AuthUser(user_id));
    let resp = next.run(req).await;
    Ok((cookies, resp).into_response())
}

fn add_signed_cookie(cookies: SignedCookieJar, uid: &str) -> SignedCookieJar {
    cookies.add(
        cookie::Cookie::build(("aos_session", uid.to_string()))
            .path("/")
            .http_only(true)
            .secure(true)
            .max_age(time::Duration::days(30))
            .same_site(cookie::SameSite::Strict)
            .build(),
    )
}
// ==================== TOKEN EXCHANGE LOGIC ====================

async fn exchange_token_with_cloud(
    cloud_api_url: &str,
    temp_token: &str,
    api_key: &SecretString,
    package_name: &str,
) -> Result<String> {
    let endpoint = format!("{cloud_api_url}/api/auth/exchange-user-token");
    let client = Client::new();
    let payload = serde_json::json!({
        "aos_temp_token": temp_token,
        "packageName": package_name,
    });
    let resp = client
        .post(endpoint)
        .json(&payload)
        .header("Content-Type", "application/json")
        .header(
            "Authorization",
            format!("Bearer {}", api_key.expose_secret()),
        )
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Network error")?;
    let status = resp.status();
    let data: serde_json::Value = resp.json().await.context("Parse error")?;
    if status == StatusCode::OK && data.get("success").and_then(|v| v.as_bool()) == Some(true) {
        if let Some(uid) = data.get("userId").and_then(|v| v.as_str()) {
            Ok(uid.to_string())
        } else {
            bail!("No userId in response")
        }
    } else {
        let msg = data
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown exchange error");
        bail!("Cloud error: {}", msg)
    }
}

fn verify_frontend_token(token: &str, api_key: &SecretString) -> Option<String> {
    let parts: Vec<&str> = token.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let token_user_id = parts[0];
    let token_hash = parts[1];
    let hashed_api_key = hex::encode(Sha256::digest(api_key.expose_secret()));
    let mut hasher = Sha256::new();
    hasher.update(token_user_id.as_bytes());
    hasher.update(hashed_api_key.as_bytes());
    let expected_hash = hex::encode(hasher.finalize());
    use subtle::ConstantTimeEq;
    if token_hash
        .as_bytes()
        .ct_eq(expected_hash.as_bytes())
        .unwrap_u8()
        == 1
    {
        Some(token_user_id.to_string())
    } else {
        None
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
                        Some(domain) if domain == config.cloud_domain => {
                            info!("WebSocket URL is valid: {}", domain)
                        }
                        _ => {
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
    user_id_for_settings: String,
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
