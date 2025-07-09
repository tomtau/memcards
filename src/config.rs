use secrecy::SecretString;

#[derive(Clone)]
pub struct AppConfig {
    pub package_name: String,
    pub api_key: SecretString,
    pub user_token_public_key: String,
    pub cloud_api_url: String,
}
