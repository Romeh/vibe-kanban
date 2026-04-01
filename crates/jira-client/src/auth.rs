use reqwest::{
    Client,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize};

use crate::{error::JiraError, types::AtlassianSite};

/// Atlassian OAuth2 endpoints.
const ATLASSIAN_TOKEN_URL: &str = "https://auth.atlassian.com/oauth/token";
const ATLASSIAN_ACCESSIBLE_RESOURCES_URL: &str =
    "https://api.atlassian.com/oauth/token/accessible-resources";

/// Authentication method for Jira Cloud.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JiraAuth {
    /// OAuth 2.0 (3LO) access token — used by Vibe Kanban Cloud.
    OAuth2 {
        access_token: String,
        refresh_token: String,
        expires_at: Option<i64>,
    },
    /// Personal API token + email — used by self-hosted instances.
    ApiToken { email: String, token: String },
}

impl JiraAuth {
    /// Build HTTP auth headers for a Jira Cloud REST API request.
    pub fn auth_headers(&self) -> Result<HeaderMap, JiraError> {
        let mut headers = HeaderMap::new();
        match self {
            JiraAuth::OAuth2 { access_token, .. } => {
                let val =
                    HeaderValue::from_str(&format!("Bearer {access_token}")).map_err(|e| {
                        JiraError::AuthFailed(format!("invalid access token header: {e}"))
                    })?;
                headers.insert(AUTHORIZATION, val);
            }
            JiraAuth::ApiToken { email, token } => {
                use base64::Engine;
                let encoded =
                    base64::engine::general_purpose::STANDARD.encode(format!("{email}:{token}"));
                let val = HeaderValue::from_str(&format!("Basic {encoded}"))
                    .map_err(|e| JiraError::AuthFailed(format!("invalid API token header: {e}")))?;
                headers.insert(AUTHORIZATION, val);
            }
        }
        Ok(headers)
    }

    /// Returns true if this is an OAuth2 token that has expired (or will expire within 60s).
    pub fn is_expired(&self) -> bool {
        match self {
            JiraAuth::OAuth2 {
                expires_at: Some(exp),
                ..
            } => chrono::Utc::now().timestamp() >= exp - 60,
            JiraAuth::OAuth2 {
                expires_at: None, ..
            } => false, // No expiry info — assume valid
            JiraAuth::ApiToken { .. } => false,
        }
    }
}

// ---------------------------------------------------------------------------
// OAuth2 token response from Atlassian
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    #[allow(dead_code)]
    token_type: Option<String>,
    #[allow(dead_code)]
    scope: Option<String>,
}

// ---------------------------------------------------------------------------
// OAuth2 functions
// ---------------------------------------------------------------------------

/// Exchange an OAuth2 authorization code for access + refresh tokens.
pub async fn exchange_oauth_code(
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<JiraAuth, JiraError> {
    let client = Client::new();
    let resp = client
        .post(ATLASSIAN_TOKEN_URL)
        .json(&serde_json::json!({
            "grant_type": "authorization_code",
            "client_id": client_id,
            "client_secret": client_secret,
            "code": code,
            "redirect_uri": redirect_uri,
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(JiraError::ApiError {
            status,
            message: format!("OAuth token exchange failed: {body}"),
        });
    }

    let token: OAuthTokenResponse = resp
        .json()
        .await
        .map_err(|e| JiraError::Parse(format!("failed to parse OAuth token response: {e}")))?;

    let expires_at = token
        .expires_in
        .map(|secs| chrono::Utc::now().timestamp() + secs);

    Ok(JiraAuth::OAuth2 {
        access_token: token.access_token,
        refresh_token: token.refresh_token.unwrap_or_else(|| {
            tracing::warn!("Atlassian OAuth response contained no refresh_token — token renewal will fail when the access token expires");
            String::new()
        }),
        expires_at,
    })
}

/// Refresh an expired OAuth2 access token. Atlassian rotates refresh tokens,
/// so the returned `JiraAuth` contains updated tokens that must be persisted.
pub async fn refresh_oauth_token(
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<JiraAuth, JiraError> {
    let client = Client::new();
    let resp = client
        .post(ATLASSIAN_TOKEN_URL)
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "client_id": client_id,
            "client_secret": client_secret,
            "refresh_token": refresh_token,
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(JiraError::AuthFailed(format!(
            "OAuth token refresh failed ({status}): {body}"
        )));
    }

    let token: OAuthTokenResponse = resp
        .json()
        .await
        .map_err(|e| JiraError::Parse(format!("failed to parse OAuth refresh response: {e}")))?;

    let expires_at = token
        .expires_in
        .map(|secs| chrono::Utc::now().timestamp() + secs);

    Ok(JiraAuth::OAuth2 {
        access_token: token.access_token,
        refresh_token: token
            .refresh_token
            .unwrap_or_else(|| refresh_token.to_string()),
        expires_at,
    })
}

#[cfg(test)]
mod tests {
    use reqwest::header::AUTHORIZATION;

    use super::*;

    // --- auth_headers ---

    #[test]
    fn test_bearer_auth_header() {
        let auth = JiraAuth::OAuth2 {
            access_token: "tok_abc123".to_string(),
            refresh_token: "ref_xyz".to_string(),
            expires_at: None,
        };
        let headers = auth.auth_headers().unwrap();
        let value = headers.get(AUTHORIZATION).unwrap().to_str().unwrap();
        assert_eq!(value, "Bearer tok_abc123");
    }

    #[test]
    fn test_api_token_auth_header() {
        let auth = JiraAuth::ApiToken {
            email: "user@example.com".to_string(),
            token: "mytoken".to_string(),
        };
        let headers = auth.auth_headers().unwrap();
        let value = headers.get(AUTHORIZATION).unwrap().to_str().unwrap();
        // base64("user@example.com:mytoken")
        use base64::Engine;
        let expected = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("user@example.com:mytoken")
        );
        assert_eq!(value, expected);
    }

    // --- is_expired ---

    #[test]
    fn test_is_expired_past_timestamp() {
        let past = chrono::Utc::now().timestamp() - 100;
        let auth = JiraAuth::OAuth2 {
            access_token: "t".to_string(),
            refresh_token: "r".to_string(),
            expires_at: Some(past),
        };
        assert!(auth.is_expired(), "100s-old token should be expired");
    }

    #[test]
    fn test_is_not_expired_future_timestamp() {
        let future = chrono::Utc::now().timestamp() + 3600;
        let auth = JiraAuth::OAuth2 {
            access_token: "t".to_string(),
            refresh_token: "r".to_string(),
            expires_at: Some(future),
        };
        assert!(
            !auth.is_expired(),
            "token expiring in 1h should not be expired"
        );
    }

    #[test]
    fn test_is_expired_within_grace_period() {
        // 30 seconds from now is within the 60s grace period → treated as expired
        let soon = chrono::Utc::now().timestamp() + 30;
        let auth = JiraAuth::OAuth2 {
            access_token: "t".to_string(),
            refresh_token: "r".to_string(),
            expires_at: Some(soon),
        };
        assert!(
            auth.is_expired(),
            "token expiring in 30s (within 60s grace) should be treated as expired"
        );
    }

    #[test]
    fn test_is_expired_no_expiry() {
        let auth = JiraAuth::OAuth2 {
            access_token: "t".to_string(),
            refresh_token: "r".to_string(),
            expires_at: None,
        };
        assert!(
            !auth.is_expired(),
            "OAuth2 with no expiry info should not be expired"
        );
    }

    #[test]
    fn test_api_token_never_expired() {
        let auth = JiraAuth::ApiToken {
            email: "a@b.com".to_string(),
            token: "tok".to_string(),
        };
        assert!(!auth.is_expired(), "API token auth never expires");
    }

    // --- serde round-trip ---

    #[test]
    fn test_oauth2_serde_roundtrip() {
        let auth = JiraAuth::OAuth2 {
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            expires_at: Some(9999999999),
        };
        let json = serde_json::to_string(&auth).unwrap();
        let decoded: JiraAuth = serde_json::from_str(&json).unwrap();
        assert!(
            matches!(decoded, JiraAuth::OAuth2 { access_token, .. } if access_token == "access")
        );
    }

    #[test]
    fn test_api_token_serde_roundtrip() {
        let auth = JiraAuth::ApiToken {
            email: "user@example.com".to_string(),
            token: "mytoken".to_string(),
        };
        let json = serde_json::to_string(&auth).unwrap();
        let decoded: JiraAuth = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, JiraAuth::ApiToken { email, .. } if email == "user@example.com"));
    }
}

/// Fetch the Atlassian Cloud sites accessible with the given OAuth token.
/// Used after code exchange to determine the user's Jira site URL.
pub async fn get_accessible_resources(access_token: &str) -> Result<Vec<AtlassianSite>, JiraError> {
    let client = Client::new();
    let resp = client
        .get(ATLASSIAN_ACCESSIBLE_RESOURCES_URL)
        .bearer_auth(access_token)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(JiraError::ApiError {
            status,
            message: format!("failed to fetch accessible resources: {body}"),
        });
    }

    resp.json()
        .await
        .map_err(|e| JiraError::Parse(format!("failed to parse accessible resources: {e}")))
}
