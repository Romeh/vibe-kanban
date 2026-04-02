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

    /// Fetch all field definitions from the Jira instance.
    /// Returns a map of lowercase field name → field ID (e.g. `"acceptance criteria"` → `"customfield_10037"`).
    #[instrument(name = "jira.get_field_ids", skip(self))]
    pub async fn get_field_ids(
        &self,
    ) -> Result<std::collections::HashMap<String, String>, JiraError> {
        #[derive(serde::Deserialize)]
        struct JiraField {
            id: String,
            name: String,
        }
        let fields: Vec<JiraField> = self.get("field").await?;
        Ok(fields
            .into_iter()
            .map(|f| (f.name.to_lowercase(), f.id))
            .collect())
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
    adf_to_markdown_inner(adf, 0)
}

fn adf_to_markdown_inner(adf: &serde_json::Value, depth: usize) -> String {
    match adf {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(obj) => {
            let node_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");

            match node_type {
                "doc" | "paragraph" => {
                    let children = extract_content_children_depth(obj, depth);
                    let text = children.join("");
                    if node_type == "paragraph" {
                        format!("{text}\n\n")
                    } else {
                        text
                    }
                }
                "text" => {
                    let raw = obj.get("text").and_then(|v| v.as_str()).unwrap_or("");
                    apply_marks(raw, obj.get("marks"))
                }
                "heading" => {
                    let level = obj
                        .get("attrs")
                        .and_then(|a| a.get("level"))
                        .and_then(|l| l.as_u64())
                        .unwrap_or(1);
                    let children = extract_content_children_depth(obj, depth);
                    let hashes = "#".repeat(level as usize);
                    format!("{hashes} {}\n\n", children.join(""))
                }
                "bulletList" => {
                    let items = extract_list_items_depth(obj, "- ", depth);
                    items.join("")
                }
                "orderedList" => {
                    let items = extract_list_items_depth(obj, "1. ", depth);
                    items.join("")
                }
                "listItem" => {
                    let children = obj
                        .get("content")
                        .and_then(|c| c.as_array())
                        .map(|arr| {
                            arr.iter()
                                .map(|child| {
                                    let child_type =
                                        child.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                    match child_type {
                                        // Nested lists get indented
                                        "bulletList" | "orderedList" => {
                                            adf_to_markdown_inner(child, depth + 1)
                                        }
                                        _ => adf_to_markdown_inner(child, depth),
                                    }
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    children.join("")
                }
                "codeBlock" => {
                    let lang = obj
                        .get("attrs")
                        .and_then(|a| a.get("language"))
                        .and_then(|l| l.as_str())
                        .unwrap_or("");
                    let children = extract_content_children_depth(obj, depth);
                    format!("```{lang}\n{}\n```\n\n", children.join(""))
                }
                "blockquote" => {
                    let children = extract_content_children_depth(obj, depth);
                    let quoted = children
                        .join("")
                        .lines()
                        .map(|l| format!("> {l}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!("{quoted}\n\n")
                }
                "hardBreak" => "\n".to_string(),
                "rule" => "---\n\n".to_string(),
                // Jira renders links/URLs as inlineCard nodes.
                "inlineCard" => {
                    let url = obj
                        .get("attrs")
                        .and_then(|a| a.get("url"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if url.is_empty() || !is_safe_url(url) {
                        String::new()
                    } else {
                        url.to_string()
                    }
                }
                // Tables: best-effort plain text.
                "table" => {
                    let children = extract_content_children_depth(obj, depth);
                    children.join("")
                }
                "tableRow" => {
                    let cells = extract_content_children_depth(obj, depth);
                    format!("| {} |\n", cells.join(" | "))
                }
                "tableHeader" | "tableCell" => {
                    let children = extract_content_children_depth(obj, depth);
                    children.join("").trim().to_string()
                }
                _ => {
                    let children = extract_content_children_depth(obj, depth);
                    children.join("")
                }
            }
        }
        _ => String::new(),
    }
}

/// Reject dangerous URL schemes that could cause XSS if markdown is rendered unsanitized.
fn is_safe_url(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    !lower.starts_with("javascript:")
        && !lower.starts_with("data:")
        && !lower.starts_with("vbscript:")
}

/// Apply ADF text marks (bold, italic, code, strikethrough, link).
/// Marks are applied in canonical order: code (innermost) → strike → em → strong → link (outermost).
fn apply_marks(text: &str, marks: Option<&serde_json::Value>) -> String {
    let Some(marks) = marks.and_then(|m| m.as_array()) else {
        return text.to_string();
    };

    let mut has_strong = false;
    let mut has_em = false;
    let mut has_strike = false;
    let mut has_code = false;
    let mut link_href: Option<String> = None;

    for mark in marks {
        let mark_type = mark.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match mark_type {
            "strong" => has_strong = true,
            "em" => has_em = true,
            "strike" => has_strike = true,
            "code" => has_code = true,
            "link" => {
                link_href = mark
                    .get("attrs")
                    .and_then(|a| a.get("href"))
                    .and_then(|v| v.as_str())
                    .filter(|href| is_safe_url(href))
                    .map(|s| s.to_string());
            }
            _ => {}
        }
    }

    // Apply innermost first: code → strike → em → strong → link
    let mut result = text.to_string();
    if has_code {
        result = format!("`{result}`");
    }
    if has_strike {
        result = format!("~~{result}~~");
    }
    if has_em {
        result = format!("*{result}*");
    }
    if has_strong {
        result = format!("**{result}**");
    }
    if let Some(href) = link_href {
        result = format!("[{result}]({href})");
    }
    result
}

fn extract_content_children_depth(
    obj: &serde_json::Map<String, serde_json::Value>,
    depth: usize,
) -> Vec<String> {
    obj.get("content")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .map(|v| adf_to_markdown_inner(v, depth))
                .collect()
        })
        .unwrap_or_default()
}

fn extract_list_items_depth(
    obj: &serde_json::Map<String, serde_json::Value>,
    prefix: &str,
    depth: usize,
) -> Vec<String> {
    let indent = "  ".repeat(depth);
    obj.get("content")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .map(|item| {
                    let text = adf_to_markdown_inner(item, depth);
                    format!("{indent}{prefix}{text}")
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

    #[test]
    fn test_adf_to_markdown_bold_and_italic() {
        let adf = serde_json::json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [
                    {
                        "type": "text",
                        "text": "bold text",
                        "marks": [{ "type": "strong" }]
                    }
                ]
            }]
        });
        let md = adf_to_markdown(&adf);
        assert!(md.contains("**bold text**"));
    }

    #[test]
    fn test_adf_to_markdown_inline_code() {
        let adf = serde_json::json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [
                    {
                        "type": "text",
                        "text": "hotfix/*",
                        "marks": [{ "type": "code" }]
                    }
                ]
            }]
        });
        let md = adf_to_markdown(&adf);
        assert!(md.contains("`hotfix/*`"));
    }

    #[test]
    fn test_adf_to_markdown_link_mark() {
        let adf = serde_json::json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [{
                    "type": "text",
                    "text": "click here",
                    "marks": [{ "type": "link", "attrs": { "href": "https://example.com" } }]
                }]
            }]
        });
        let md = adf_to_markdown(&adf);
        assert!(md.contains("[click here](https://example.com)"));
    }

    #[test]
    fn test_adf_to_markdown_inline_card() {
        let adf = serde_json::json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [{
                    "type": "inlineCard",
                    "attrs": { "url": "https://github.com/org/repo" }
                }]
            }]
        });
        let md = adf_to_markdown(&adf);
        assert!(md.contains("https://github.com/org/repo"));
    }

    #[test]
    fn test_adf_to_markdown_nested_list() {
        let adf = serde_json::json!({
            "type": "doc",
            "content": [{
                "type": "bulletList",
                "content": [{
                    "type": "listItem",
                    "content": [
                        { "type": "paragraph", "content": [{ "type": "text", "text": "parent" }] },
                        {
                            "type": "bulletList",
                            "content": [{
                                "type": "listItem",
                                "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "child" }] }]
                            }]
                        }
                    ]
                }]
            }]
        });
        let md = adf_to_markdown(&adf);
        assert!(md.contains("- parent"));
        assert!(md.contains("  - child"));
    }
}
