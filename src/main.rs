use std::env;

use anyhow::Context;
use axum_extra::extract::cookie::Key;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub(crate) mod config;
pub(crate) mod errors;
mod import;
pub(crate) mod models;
mod router;
mod routes;
pub(crate) mod sdk;
pub(crate) mod srs;
mod templates;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "memcards=debug,tower_http=debug".parse().unwrap()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration from environment variables
    let database_url = env::var("DATABASE_URL").context("DATABASE_URL must be set")?;

    // Create database connection pool
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .context("Failed to connect to database")?;

    // Run migrations
    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let cloud_api_url = env::var("CLOUD_API_URL")
        .unwrap_or_else(|_| "https://prod.augmentos.cloud".to_string());
    let cloud_domain = cloud_api_url
        .strip_prefix("https://")
        .or(cloud_api_url.strip_prefix("http://"))
        .unwrap_or(&cloud_api_url)
        .to_string();
    let default_user_token_public_key = "-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA0Yt2RtNOdeKQxWMY0c84\nADpY1Jy58YWZhaEgP2A5tBwFUKgy/TH9gQLWZjQ3dQ/6XXO8qq0kluoYFqM7ZDRF\nzJ0E4Yi0WQncioLRcCx4q8pDmqY9vPKgv6PruJdFWca0l0s3gZ3BqSeWum/C23xK\nFPHPwi8gvRdc6ALrkcHeciM+7NykU8c0EY8PSitNL+Tchti95kGu+j6APr5vNewi\nzRpQGOdqaLWe+ahHmtj6KtUZjm8o6lan4f/o08C6litizguZXuw2Nn/Kd9fFI1xF\nIVNJYMy9jgGaOi71+LpGw+vIpwAawp/7IvULDppvY3DdX5nt05P1+jvVJXPxMKzD\nTQIDAQAB\n-----END PUBLIC KEY-----".to_string();
    let package_name = env::var("PACKAGE_NAME").context("PACKAGE_NAME must be set")?;
    let api_key = env::var("API_KEY").context("API_KEY must be set")?.into();
    let cookie_secret = Key::generate();
    let config = config::AppConfig {
        package_name,
        api_key,
        cookie_secret,
        user_token_public_key: env::var("USER_TOKEN_PUBLIC_KEY")
            .unwrap_or(default_user_token_public_key),
        cloud_api_url,
        cloud_domain,
    };

    let router = router::init_router(pool, config);

    // Get the host and port from environment variables or use defaults
    let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("PORT").unwrap_or_else(|_| "8000".to_string());
    let addr = format!("{}:{}", host, port);

    info!("Starting server on {}", addr);

    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
