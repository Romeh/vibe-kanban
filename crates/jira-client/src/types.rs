use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ---------------------------------------------------------------------------
// Jira REST API response types
// ---------------------------------------------------------------------------

/// A Jira project summary as returned by `/rest/api/3/project`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct JiraProject {
    pub id: String,
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub project_type_key: Option<String>,
}

/// A Jira issue as returned by `/rest/api/3/issue/{key}`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraIssue {
    pub id: String,
    pub key: String,
    #[serde(rename = "self")]
    pub self_url: String,
    pub fields: JiraIssueFields,
}

/// The `fields` object inside a Jira issue response.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraIssueFields {
    pub summary: String,
    pub description: Option<serde_json::Value>,
    pub status: Option<JiraStatus>,
    pub priority: Option<JiraPriority>,
    pub issuetype: Option<JiraIssueType>,
    pub assignee: Option<JiraUser>,
    pub reporter: Option<JiraUser>,
    pub created: Option<String>,
    pub updated: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    /// Catch-all for custom fields (e.g. acceptance criteria, story points).
    #[serde(flatten)]
    #[ts(skip)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraStatus {
    pub id: String,
    pub name: String,
    #[serde(rename = "statusCategory")]
    pub status_category: Option<JiraStatusCategory>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraStatusCategory {
    pub id: i64,
    pub key: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraPriority {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraIssueType {
    pub id: String,
    pub name: String,
    pub subtask: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct JiraUser {
    pub account_id: String,
    pub display_name: String,
}

/// A Jira status transition.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraTransition {
    pub id: String,
    pub name: String,
    pub to: JiraStatus,
}

// ---------------------------------------------------------------------------
// Search results
// ---------------------------------------------------------------------------

/// Search response from `/rest/api/3/search/jql`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct JiraSearchResult {
    #[serde(default)]
    pub start_at: Option<i64>,
    #[serde(default)]
    pub max_results: Option<i64>,
    #[serde(default)]
    pub total: Option<i64>,
    pub issues: Vec<JiraIssue>,
    #[serde(default)]
    pub is_last: Option<bool>,
}

// ---------------------------------------------------------------------------
// Request types for Vibe Kanban API
// ---------------------------------------------------------------------------

/// Request to connect a Jira instance to an organization.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraConnectRequest {
    pub site_url: String,
    pub auth: JiraAuthPayload,
}

/// Auth payload sent from the frontend when connecting Jira.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JiraAuthPayload {
    /// OAuth2 flow completed — frontend sends the authorization code.
    OAuth2 { code: String, redirect_uri: String },
    /// API token provided directly.
    ApiToken { email: String, token: String },
}

/// Request to import Jira issues into a VK project.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraImportRequest {
    pub project_id: uuid::Uuid,
    pub status_id: uuid::Uuid,
    pub issue_keys: Vec<String>,
}

/// Summary of a Jira connection (returned to the frontend, no secrets).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraConnectionInfo {
    pub connected: bool,
    pub site_url: Option<String>,
    pub auth_type: Option<String>,
    pub connected_at: Option<String>,
}

/// Request to search Jira issues.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraSearchRequest {
    pub query: String,
    pub project_key: Option<String>,
    pub max_results: Option<u32>,
}

// ---------------------------------------------------------------------------
// Atlassian OAuth types
// ---------------------------------------------------------------------------

/// An Atlassian Cloud site accessible via OAuth token.
/// Returned by the `/oauth/token/accessible-resources` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AtlassianSite {
    pub id: String,
    pub url: String,
    pub name: String,
}

/// Response from the `/v1/jira/oauth/authorize` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraOAuthAuthorizeResponse {
    pub authorize_url: String,
}

// ---------------------------------------------------------------------------
// Jira comment types
// ---------------------------------------------------------------------------

/// A single Jira issue comment.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraComment {
    pub id: String,
    pub author: Option<JiraUser>,
    /// ADF document body (Atlassian Document Format).
    pub body: Option<serde_json::Value>,
    pub created: Option<String>,
    pub updated: Option<String>,
}

/// Paginated response from the Jira comments endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraCommentResponse {
    pub comments: Vec<JiraComment>,
    pub total: Option<i64>,
    pub max_results: Option<i64>,
    pub start_at: Option<i64>,
}

/// A VK-friendly comment with markdown body (returned to the frontend).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraCommentView {
    pub id: String,
    pub author_name: String,
    pub body_markdown: String,
    pub created: Option<String>,
}

// ---------------------------------------------------------------------------
// Status mapping types
// ---------------------------------------------------------------------------

/// A VK status → Jira category mapping.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraStatusMapping {
    pub vk_status_name: String,
    pub jira_category_key: String,
}

/// Request to upsert a status mapping.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraStatusMappingRequest {
    pub vk_status_name: String,
    pub jira_category_key: String,
}

/// Request to delete a status mapping.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraStatusMappingDeleteRequest {
    pub vk_status_name: String,
}

/// A Jira status with its category, returned to the frontend for the mapping dropdown.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct JiraStatusView {
    pub name: String,
    pub category_key: String,
    pub category_name: String,
}
