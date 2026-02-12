use anyhow::{Context, Result};
use secrecy::SecretString;

/// Fetch a fresh GCP OAuth2 token using Application Default Credentials.
#[allow(dead_code)]
pub async fn get_token() -> Result<SecretString> {
    let auth_manager = gcp_auth::provider()
        .await
        .context("Failed to initialize GCP auth provider")?;

    let token = auth_manager
        .token(&["https://www.googleapis.com/auth/cloud-platform"])
        .await
        .context("Failed to get GCP OAuth token")?;

    let token_str = token.as_str().to_string();

    Ok(SecretString::from(token_str))
}
