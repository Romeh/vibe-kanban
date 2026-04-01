pub mod auth;
pub mod client;
pub mod error;
pub mod types;

pub use auth::{JiraAuth, exchange_oauth_code, get_accessible_resources, refresh_oauth_token};
pub use client::{JiraClient, adf_to_markdown};
pub use error::JiraError;
pub use types::*;

/// Validate that a Jira site URL points to a legitimate Atlassian Cloud instance.
/// Rejects private/internal IPs and non-Atlassian hostnames to prevent SSRF.
pub fn validate_jira_site_url(url: &str) -> Result<(), &'static str> {
    if !url.starts_with("https://") {
        return Err("site_url must use HTTPS");
    }

    // Extract hostname from URL (strip scheme and path).
    let after_scheme = &url["https://".len()..];
    let host = after_scheme
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_lowercase();

    if host.is_empty() {
        return Err("site_url must contain a hostname");
    }

    // Must be an Atlassian Cloud domain.
    if !host.ends_with(".atlassian.net") && !host.ends_with(".atlassian.com") {
        return Err(
            "site_url must be an Atlassian Cloud domain (*.atlassian.net or *.atlassian.com)",
        );
    }

    Ok(())
}
