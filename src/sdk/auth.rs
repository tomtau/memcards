//! Authentication middleware and token verification logic.
use crate::{config::AppConfig, router::AppState, sdk::app_session::UserId};
use anyhow::{Context, Result, bail};
use axum::{
    Extension,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_extra::extract::{SignedCookieJar, cookie};
use jsonwebtoken::{Algorithm, DecodingKey, TokenData, Validation, decode};
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, sync::Arc, time::Duration};

use tracing::{debug, info, warn};
pub(crate) fn verify_signed_user_token(token: &str, public_key_pem: &str) -> Result<UserId> {
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
        .map(|s| s.to_string().into())
        .ok_or_else(|| anyhow::anyhow!("No 'sub' claim in JWT"))
}

#[derive(Clone, Debug)]
pub struct AuthUser(pub Option<UserId>);

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
    let mut user_id: Option<UserId> = None;
    let headers = req.headers();
    let mut cookies = SignedCookieJar::from_headers(headers, config.cookie_secret.clone());

    debug!("Headers: {:?}", headers);
    debug!("Query string: {:?}", req.uri().query());

    // --- 1. JWT Signed User Token (query param) ---
    if let Some(signed_user_token) = get_query_param(req.uri().query(), "aos_signed_user_token") {
        match verify_signed_user_token(&signed_user_token, &config.user_token_public_key) {
            Ok(uid) => {
                user_id = Some(uid.clone());
                cookies = add_signed_cookie(cookies, &uid.0);
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
                cookies = add_signed_cookie(cookies, &uid.0);
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
                        cookies = add_signed_cookie(cookies, &uid.0);
                        info!(
                            "User ID verified from frontend token in Authorization header: {}",
                            uid
                        );
                        user_id = Some(uid);
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
                cookies = add_signed_cookie(cookies, &uid.0);
                info!("User ID verified from temporary token: {}", uid);
                user_id = Some(uid);
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
                cookies = add_signed_cookie(cookies, &uid.0);
                info!("User ID verified from frontend user token: {}", uid);
                user_id = Some(uid);
            }
            None => {
                warn!("Frontend token invalid");
            }
        }
    }
    // --- 5. Session Cookie ---
    else if let Some(cookie) = cookies.get("aos_session") {
        let uid = cookie.value().to_string().into();
        info!("Session cookie found: {}", uid);
        user_id = Some(uid);
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
) -> Result<UserId> {
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
            Ok(uid.to_string().into())
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

fn verify_frontend_token(token: &str, api_key: &SecretString) -> Option<UserId> {
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
        Some(token_user_id.to_string().into())
    } else {
        None
    }
}
