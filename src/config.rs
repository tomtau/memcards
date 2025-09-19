//! Application configuration structure
use axum_extra::extract::cookie::Key;
use secrecy::SecretString;

#[derive(Clone)]
pub struct AppConfig {
    pub package_name: String,
    pub api_key: SecretString,
    pub cookie_secret: Key,
    pub user_token_public_key: String,
    pub cloud_api_url: String,
    pub cloud_domain: String,
}
