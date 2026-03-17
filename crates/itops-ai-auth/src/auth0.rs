use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::time::{Duration, SystemTime};
use tracing::{debug, error};

#[derive(Clone)]
pub struct Auth0TokenProvider {
    http: Client,
    domain: String,
    client_id: String,
    audience: String,
    refresh_token: String,
    access_token: Option<String>,
    expires_at: Option<SystemTime>,
}

#[derive(Debug, Deserialize)]
struct TokenRes {
    access_token: String,
    expires_in: u64,
    // id_token, scope, token_type etc. optional
}

impl Auth0TokenProvider {
    pub fn new(
        domain: String,
        client_id: String,
        audience: String,
        refresh_token: String,
    ) -> Self {
        Self {
            http: Client::new(),
            domain,
            client_id,
            audience,
            refresh_token,
            access_token: None,
            expires_at: None,
        }
    }

    async fn refresh(&mut self) -> Result<()> {
        let url = format!("https://{}/oauth/token", self.domain);
        let body = serde_json::json!({
          "grant_type": "refresh_token",
          "client_id": self.client_id,
          "refresh_token": self.refresh_token,
          "audience": self.audience
        });

        debug!("Refreshing Auth0 token from {}", url);

        let res = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("auth0 token request failed")?;

        if !res.status().is_success() {
            let t = res.text().await.unwrap_or_default();
            error!("Auth0 token exchange failed: {}", t);
            anyhow::bail!("auth0 token exchange failed: {}", t);
        }

        let tr: TokenRes = res.json().await.context("parse token response")?;
        self.access_token = Some(tr.access_token);
        self.expires_at =
            Some(SystemTime::now() + Duration::from_secs(tr.expires_in.saturating_sub(30)));
        debug!("Auth0 token refreshed successfully");
        Ok(())
    }

    pub async fn get_bearer(&mut self) -> Result<String> {
        let needs_refresh = match self.expires_at {
            None => true,
            Some(t) => SystemTime::now() >= t,
        };
        if needs_refresh {
            self.refresh().await?;
        }
        self.access_token
            .as_ref()
            .map(|token| format!("Bearer {token}"))
            .context("no access token available after refresh")
    }
}
