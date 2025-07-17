use anyhow::Context;
use axum_extra::extract::cookie::Key;
use shuttle_runtime::SecretStore;
use sqlx::PgPool;
pub(crate) mod config;
pub(crate) mod errors;
mod import;
pub(crate) mod models;
mod router;
mod routes;
pub(crate) mod sdk;
pub(crate) mod srs;
mod templates;

#[shuttle_runtime::main]
async fn main(
    #[shuttle_shared_db::Postgres] pool: PgPool,
    #[shuttle_runtime::Secrets] secrets: SecretStore,
) -> shuttle_axum::ShuttleAxum {
    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let cloud_api_url = secrets
        .get("CLOUD_API_URL")
        .unwrap_or_else(|| "https://prod.augmentos.cloud".to_string());
    let cloud_domain = cloud_api_url
        .strip_prefix("https://")
        .or(cloud_api_url.strip_prefix("http://"))
        .unwrap_or(&cloud_api_url)
        .to_string();
    let default_user_token_public_key = "-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA0Yt2RtNOdeKQxWMY0c84\nADpY1Jy58YWZhaEgP2A5tBwFUKgy/TH9gQLWZjQ3dQ/6XXO8qq0kluoYFqM7ZDRF\nzJ0E4Yi0WQncioLRcCx4q8pDmqY9vPKgv6PruJdFWca0l0s3gZ3BqSeWum/C23xK\nFPHPwi8gvRdc6ALrkcHeciM+7NykU8c0EY8PSitNL+Tchti95kGu+j6APr5vNewi\nzRpQGOdqaLWe+ahHmtj6KtUZjm8o6lan4f/o08C6litizguZXuw2Nn/Kd9fFI1xF\nIVNJYMy9jgGaOi71+LpGw+vIpwAawp/7IvULDppvY3DdX5nt05P1+jvVJXPxMKzD\nTQIDAQAB\n-----END PUBLIC KEY-----".to_string();
    let package_name = secrets
        .get("PACKAGE_NAME")
        .context("PACKAGE_NAME not found")?;
    let api_key = secrets.get("API_KEY").context("API_KEY not found")?.into();
    let cookie_secret = Key::generate();
    let config = config::AppConfig {
        package_name,
        api_key,
        cookie_secret,
        user_token_public_key: secrets
            .get("USER_TOKEN_PUBLIC_KEY")
            .unwrap_or(default_user_token_public_key),
        cloud_api_url,
        cloud_domain,
    };

    let router = router::init_router(pool, config);

    Ok(router.into())
}
