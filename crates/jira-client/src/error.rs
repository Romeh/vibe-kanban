use thiserror::Error;

#[derive(Debug, Error)]
pub enum JiraError {
    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("Insufficient permissions: {0}")]
    InsufficientPermissions(String),

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Rate limited by Jira API")]
    RateLimited,

    #[error("Jira API error ({status}): {message}")]
    ApiError { status: u16, message: String },

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Failed to parse response: {0}")]
    Parse(String),

    #[error("No Jira connection configured")]
    NotConfigured,
}

impl JiraError {
    /// Whether this error is transient and the operation should be retried.
    pub fn should_retry(&self) -> bool {
        matches!(
            self,
            JiraError::RateLimited
                | JiraError::Network(_)
                | JiraError::ApiError {
                    status: 502..=504,
                    ..
                }
        )
    }
}
