use reqwest::Client;
use tracing::instrument;

use crate::{
    auth::JiraAuth,
    error::JiraError,
    types::{JiraIssue, JiraProject, JiraSearchResult, JiraStatus, JiraTransition},
};

/// HTTP client for the Jira Cloud REST API v3.
#[derive(Debug, Clone)]
pub struct JiraClient {
    http: Client,
    site_url: String,
    auth: JiraAuth,
}

impl JiraClient {
    /// Create a new Jira client for the given site and auth credentials.
    ///
    /// `site_url` should be the Atlassian site root, e.g. `https://mycompany.atlassian.net`.
    pub fn new(site_url: impl Into<String>, auth: JiraAuth) -> Self {
        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to build HTTP client"),
            site_url: site_url.into().trim_end_matches('/').to_string(),
            auth,
        }
    }

    /// Build a full URL for a Jira REST API v3 endpoint.
    fn api_url(&self, path: &str) -> String {
        format!(
            "{}/rest/api/3/{}",
            self.site_url,
            path.trim_start_matches('/')
        )
    }

    /// Execute a GET request and deserialize the response.
    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, JiraError> {
        let resp = self
            .http
            .get(self.api_url(path))
            .headers(self.auth.auth_headers()?)
            .send()
            .await?;

        Self::handle_response(resp).await
    }

    /// Execute a POST request with a JSON body and deserialize the response.
    #[allow(dead_code)]
    async fn post<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, JiraError> {
        let resp = self
            .http
            .post(self.api_url(path))
            .headers(self.auth.auth_headers()?)
            .json(body)
            .send()
            .await?;

        Self::handle_response(resp).await
    }

    /// Map HTTP response status codes to typed errors.
    async fn handle_response<T: serde::de::DeserializeOwned>(
        resp: reqwest::Response,
    ) -> Result<T, JiraError> {
        let status = resp.status().as_u16();
        if resp.status().is_success() {
            resp.json::<T>()
                .await
                .map_err(|e| JiraError::Parse(e.to_string()))
        } else {
            let message = resp
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            match status {
                401 => Err(JiraError::AuthFailed(message)),
                403 => Err(JiraError::InsufficientPermissions(message)),
                404 => Err(JiraError::NotFound(message)),
                429 => Err(JiraError::RateLimited),
                _ => Err(JiraError::ApiError { status, message }),
            }
        }
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// List all projects accessible to the authenticated user.
    #[instrument(name = "jira.get_projects", skip(self))]
    pub async fn get_projects(&self) -> Result<Vec<JiraProject>, JiraError> {
        self.get("project?expand=description&recent=50").await
    }

    /// Fetch a single issue by key (e.g. `PROJ-123`).
    #[instrument(name = "jira.get_issue", skip(self), fields(%issue_key))]
    pub async fn get_issue(&self, issue_key: &str) -> Result<JiraIssue, JiraError> {
        self.get(&format!("issue/{issue_key}")).await
    }

    /// Fetch all statuses available in the Jira instance.
    #[instrument(name = "jira.get_statuses", skip(self))]
    pub async fn get_statuses(&self) -> Result<Vec<JiraStatus>, JiraError> {
        // Try paginated endpoint first (newer Jira Cloud API)
        #[derive(serde::Deserialize)]
        struct PaginatedStatuses {
            values: Vec<JiraStatus>,
        }

        match self
            .get::<PaginatedStatuses>("statuses/search?maxResults=200")
            .await
        {
            Ok(paginated) => Ok(paginated.values),
            Err(JiraError::NotFound(_)) | Err(JiraError::ApiError { status: 404, .. }) => {
                tracing::debug!("paginated statuses endpoint not found, falling back to legacy");
                self.get("status").await
            }
            Err(JiraError::Parse(_)) => {
                // Response format doesn't match paginated schema — try legacy endpoint
                tracing::debug!("paginated statuses parse failed, falling back to legacy");
                self.get("status").await
            }
            Err(e) => Err(e),
        }
    }

    /// Search issues using JQL.
    /// Uses the `/rest/api/3/search/jql` endpoint (the legacy `/search` was removed).
    #[instrument(name = "jira.search_issues", skip(self), fields(%jql, max_results))]
    pub async fn search_issues(
        &self,
        jql: &str,
        max_results: u32,
    ) -> Result<JiraSearchResult, JiraError> {
        let fields = "summary,description,status,priority,issuetype,assignee,reporter,created,updated,labels";
        let encoded_jql = urlencoding::encode(jql);
        let path = format!(
            "search/jql?jql={}&maxResults={}&fields={}",
            encoded_jql, max_results, fields
        );
        self.get(&path).await
    }

    /// List available transitions for an issue.
    #[instrument(name = "jira.get_transitions", skip(self), fields(%issue_key))]
    pub async fn get_transitions(&self, issue_key: &str) -> Result<Vec<JiraTransition>, JiraError> {
        #[derive(serde::Deserialize)]
        struct Wrapper {
            transitions: Vec<JiraTransition>,
        }
        let wrapper: Wrapper = self.get(&format!("issue/{issue_key}/transitions")).await?;
        Ok(wrapper.transitions)
    }

    /// Transition an issue to a new status.
    #[instrument(name = "jira.transition_issue", skip(self), fields(%issue_key, %transition_id))]
    pub async fn transition_issue(
        &self,
        issue_key: &str,
        transition_id: &str,
    ) -> Result<(), JiraError> {
        let body = serde_json::json!({
            "transition": { "id": transition_id }
        });
        let resp = self
            .http
            .post(self.api_url(&format!("issue/{issue_key}/transitions")))
            .headers(self.auth.auth_headers()?)
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        if resp.status().is_success() {
            Ok(())
        } else {
            let message = resp.text().await.unwrap_or_default();
            match status {
                401 => Err(JiraError::AuthFailed(message)),
                403 => Err(JiraError::InsufficientPermissions(message)),
                404 => Err(JiraError::NotFound(message)),
                429 => Err(JiraError::RateLimited),
                _ => Err(JiraError::ApiError { status, message }),
            }
        }
    }

    /// Fetch comments for a Jira issue, most recent first.
    #[instrument(name = "jira.get_comments", skip(self), fields(%issue_key))]
    pub async fn get_comments(
        &self,
        issue_key: &str,
    ) -> Result<crate::types::JiraCommentResponse, JiraError> {
        self.get(&format!(
            "issue/{issue_key}/comment?orderBy=-created&maxResults=50"
        ))
        .await
    }

    /// Add a plain-text comment to a Jira issue (wrapped in ADF).
    #[instrument(name = "jira.add_comment", skip(self, body_text), fields(%issue_key))]
    pub async fn add_comment(&self, issue_key: &str, body_text: &str) -> Result<(), JiraError> {
        // Build ADF document with one paragraph per line.
        let paragraphs: Vec<serde_json::Value> = body_text
            .lines()
            .map(|line| {
                serde_json::json!({
                    "type": "paragraph",
                    "content": [{ "type": "text", "text": line }]
                })
            })
            .collect();

        let adf_body = serde_json::json!({
            "body": {
                "version": 1,
                "type": "doc",
                "content": paragraphs
            }
        });

        let resp = self
            .http
            .post(self.api_url(&format!("issue/{issue_key}/comment")))
            .headers(self.auth.auth_headers()?)
            .json(&adf_body)
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let message = resp.text().await.unwrap_or_default();
            Err(JiraError::ApiError { status, message })
        }
    }

    /// Get the site URL for this client.
    pub fn site_url(&self) -> &str {
        &self.site_url
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert Jira ADF (Atlassian Document Format) JSON to plain-text markdown.
/// This is a best-effort conversion for importing issue descriptions.
pub fn adf_to_markdown(adf: &serde_json::Value) -> String {
    match adf {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(obj) => {
            let node_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");

            match node_type {
                "doc" | "paragraph" => {
                    let children = extract_content_children(obj);
                    let text = children.join("");
                    if node_type == "paragraph" {
                        format!("{text}\n\n")
                    } else {
                        text
                    }
                }
                "text" => obj
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                "heading" => {
                    let level = obj
                        .get("attrs")
                        .and_then(|a| a.get("level"))
                        .and_then(|l| l.as_u64())
                        .unwrap_or(1);
                    let children = extract_content_children(obj);
                    let hashes = "#".repeat(level as usize);
                    format!("{hashes} {}\n\n", children.join(""))
                }
                "bulletList" => {
                    let items = extract_list_items(obj, "- ");
                    items.join("")
                }
                "orderedList" => {
                    let items = extract_list_items(obj, "1. ");
                    items.join("")
                }
                "listItem" => {
                    let children = extract_content_children(obj);
                    children.join("")
                }
                "codeBlock" => {
                    let lang = obj
                        .get("attrs")
                        .and_then(|a| a.get("language"))
                        .and_then(|l| l.as_str())
                        .unwrap_or("");
                    let children = extract_content_children(obj);
                    format!("```{lang}\n{}\n```\n\n", children.join(""))
                }
                "blockquote" => {
                    let children = extract_content_children(obj);
                    let quoted = children
                        .join("")
                        .lines()
                        .map(|l| format!("> {l}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!("{quoted}\n\n")
                }
                "hardBreak" => "\n".to_string(),
                _ => {
                    let children = extract_content_children(obj);
                    children.join("")
                }
            }
        }
        _ => String::new(),
    }
}

fn extract_content_children(obj: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    obj.get("content")
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().map(adf_to_markdown).collect())
        .unwrap_or_default()
}

fn extract_list_items(
    obj: &serde_json::Map<String, serde_json::Value>,
    prefix: &str,
) -> Vec<String> {
    obj.get("content")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .map(|item| {
                    let text = adf_to_markdown(item);
                    format!("{prefix}{text}")
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adf_to_markdown_plain_text() {
        let adf = serde_json::json!({
            "type": "doc",
            "content": [
                {
                    "type": "paragraph",
                    "content": [
                        { "type": "text", "text": "Hello world" }
                    ]
                }
            ]
        });
        let md = adf_to_markdown(&adf);
        assert!(md.contains("Hello world"));
    }

    #[test]
    fn test_adf_to_markdown_heading() {
        let adf = serde_json::json!({
            "type": "doc",
            "content": [
                {
                    "type": "heading",
                    "attrs": { "level": 2 },
                    "content": [
                        { "type": "text", "text": "Section Title" }
                    ]
                }
            ]
        });
        let md = adf_to_markdown(&adf);
        assert!(md.contains("## Section Title"));
    }

    #[test]
    fn test_adf_to_markdown_string_passthrough() {
        let adf = serde_json::Value::String("plain text description".into());
        assert_eq!(adf_to_markdown(&adf), "plain text description");
    }
}
