use api_types::{CreateIssueRequest, Issue, IssuePriority};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::Json as ResponseJson,
    routing::{delete, get, post},
};
use db::models::{
    jira::{JiraConnectionRow, StatusMappingRow},
    local_issue::LocalIssue,
    project_status::LocalProjectStatus,
    task::Task,
};
use deployment::Deployment;
use jira_client::{
    JiraAuth, JiraClient, JiraCommentView, JiraConnectRequest, JiraConnectionInfo,
    JiraImportRequest, JiraOAuthAuthorizeResponse, JiraSearchRequest, JiraSearchResult,
    JiraStatusMapping, JiraStatusMappingDeleteRequest, JiraStatusMappingRequest, JiraStatusView,
    adf_to_markdown, types::JiraProject,
};
use serde_json::json;
use tracing::instrument;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, crypto::LocalCrypto, error::ApiError};

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/jira/connection", get(get_connection))
        .route("/jira/connect", post(connect))
        .route("/jira/connect", delete(disconnect))
        .route("/jira/projects", get(list_projects))
        .route("/jira/search", post(search_issues))
        .route("/jira/import", post(import_issues))
        .route("/jira/sync-status/{issue_id}", post(manual_sync_status))
        .route("/jira/comments/{issue_id}", get(get_jira_comments))
        .route("/jira/post-summary/{issue_id}", post(post_summary_to_jira))
        .route("/jira/statuses", get(get_jira_statuses))
        .route("/jira/status-mappings", get(get_status_mappings))
        .route("/jira/status-mappings/upsert", post(upsert_status_mapping))
        .route("/jira/status-mappings/delete", post(delete_status_mapping))
        .route("/jira/oauth/authorize", get(oauth_authorize_stub))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn build_jira_client(deployment: &DeploymentImpl) -> Result<JiraClient, ApiError> {
    let row = JiraConnectionRow::find(&deployment.db().pool)
        .await?
        .ok_or_else(|| ApiError::Jira("No Jira connection configured".into()))?;

    let crypto = LocalCrypto::new(deployment.user_id());
    let decrypted = crypto.decrypt(&row.encrypted_credentials)?;
    let auth: JiraAuth = serde_json::from_slice(&decrypted)?;

    Ok(JiraClient::new(&row.jira_site_url, auth))
}

fn is_issue_key(s: &str) -> bool {
    let Some((prefix, number)) = s.split_once('-') else {
        return false;
    };
    !prefix.is_empty()
        && prefix.chars().all(|c| c.is_ascii_uppercase())
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

fn map_jira_priority(name: &str) -> Option<IssuePriority> {
    match name.to_lowercase().as_str() {
        "highest" | "blocker" => Some(IssuePriority::Urgent),
        "high" | "critical" => Some(IssuePriority::High),
        "medium" => Some(IssuePriority::Medium),
        "low" | "lowest" | "trivial" => Some(IssuePriority::Low),
        _ => None,
    }
}

fn map_status_to_jira_category(status_name: &str) -> Option<&'static str> {
    match status_name.to_lowercase().as_str() {
        "done" | "cancelled" | "canceled" => Some("done"),
        "inprogress" | "in progress" | "in_progress" => Some("indeterminate"),
        "inreview" | "in review" | "in_review" => Some("indeterminate"),
        "todo" | "to do" | "backlog" => Some("new"),
        _ => None,
    }
}

/// Update fields in extension_metadata.jira for an entity (Issue or Task).
async fn update_jira_metadata(
    pool: &sqlx::SqlitePool,
    entity_id: Uuid,
    current_metadata: &serde_json::Value,
    fields: &[(&str, serde_json::Value)],
    is_issue: bool,
) {
    let mut metadata = current_metadata.clone();
    if let Some(jira) = metadata.get_mut("jira").and_then(|v| v.as_object_mut()) {
        for (key, value) in fields {
            jira.insert((*key).to_string(), value.clone());
        }
    }

    let metadata_str = metadata.to_string();
    let result = if is_issue {
        sqlx::query(
            "UPDATE issues SET extension_metadata = $1, updated_at = datetime('now', 'subsec') WHERE id = $2",
        )
        .bind(&metadata_str)
        .bind(entity_id)
        .execute(pool)
        .await
    } else {
        sqlx::query(
            "UPDATE tasks SET extension_metadata = $1, updated_at = datetime('now', 'subsec') WHERE id = $2",
        )
        .bind(&metadata_str)
        .bind(entity_id)
        .execute(pool)
        .await
    };

    if let Err(e) = result {
        tracing::warn!(?e, %entity_id, "failed to update jira metadata");
    }
}

// ---------------------------------------------------------------------------
// Connection handlers
// ---------------------------------------------------------------------------

#[instrument(name = "jira.get_connection", skip(deployment))]
async fn get_connection(
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<JiraConnectionInfo>, ApiError> {
    let row = JiraConnectionRow::find(&deployment.db().pool).await?;

    let info = match row {
        Some(r) => JiraConnectionInfo {
            connected: true,
            site_url: Some(r.jira_site_url),
            auth_type: Some(r.auth_type),
            connected_at: Some(r.created_at.to_rfc3339()),
        },
        None => JiraConnectionInfo {
            connected: false,
            site_url: None,
            auth_type: None,
            connected_at: None,
        },
    };

    Ok(ResponseJson(info))
}

#[instrument(name = "jira.connect", skip(deployment, payload))]
async fn connect(
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<JiraConnectRequest>,
) -> Result<ResponseJson<JiraConnectionInfo>, ApiError> {
    let site_url = payload.site_url.trim().trim_end_matches('/').to_string();

    if let Err(msg) = jira_client::validate_jira_site_url(&site_url) {
        return Err(ApiError::BadRequest(msg.into()));
    }

    // Only API token auth is supported in local mode (v1).
    let (auth, auth_type) = match payload.auth {
        jira_client::JiraAuthPayload::ApiToken { email, token } => {
            let auth = JiraAuth::ApiToken {
                email: email.clone(),
                token: token.clone(),
            };
            (auth, "api_token")
        }
        jira_client::JiraAuthPayload::OAuth2 { .. } => {
            return Err(ApiError::BadRequest(
                "OAuth is not supported in local mode. Use API token authentication.".into(),
            ));
        }
    };

    // Verify credentials by fetching projects.
    let client = JiraClient::new(&site_url, auth.clone());
    client.get_projects().await.map_err(|e| {
        tracing::warn!(?e, "jira credential verification failed");
        ApiError::BadRequest(
            "Failed to connect to Jira — check your credentials and site URL".into(),
        )
    })?;

    // Encrypt and store.
    let auth_json = serde_json::to_vec(&auth)?;
    let crypto = LocalCrypto::new(deployment.user_id());
    let encrypted = crypto.encrypt(&auth_json)?;

    let row =
        JiraConnectionRow::upsert(&deployment.db().pool, &site_url, auth_type, &encrypted).await?;

    Ok(ResponseJson(JiraConnectionInfo {
        connected: true,
        site_url: Some(row.jira_site_url),
        auth_type: Some(row.auth_type),
        connected_at: Some(row.created_at.to_rfc3339()),
    }))
}

#[instrument(name = "jira.disconnect", skip(deployment))]
async fn disconnect(State(deployment): State<DeploymentImpl>) -> Result<StatusCode, ApiError> {
    JiraConnectionRow::delete(&deployment.db().pool).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Project & search handlers
// ---------------------------------------------------------------------------

#[instrument(name = "jira.list_projects", skip(deployment))]
async fn list_projects(
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<Vec<JiraProject>>, ApiError> {
    let client = build_jira_client(&deployment).await?;
    let projects = client.get_projects().await?;
    Ok(ResponseJson(projects))
}

#[instrument(name = "jira.search_issues", skip(deployment, payload))]
async fn search_issues(
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<JiraSearchRequest>,
) -> Result<ResponseJson<JiraSearchResult>, ApiError> {
    let client = build_jira_client(&deployment).await?;

    let trimmed = payload.query.trim();
    let jql = if trimmed.contains('=') || trimmed.contains("ORDER BY") {
        trimmed.to_string()
    } else if is_issue_key(trimmed) {
        format!("key = \"{trimmed}\"")
    } else {
        let escaped = trimmed.replace('"', "\\\"");
        let mut jql = format!("text ~ \"{escaped}\"");
        if let Some(ref project_key) = payload.project_key {
            if project_key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                jql = format!("project = \"{project_key}\" AND {jql}");
            }
        }
        jql.push_str(" ORDER BY updated DESC");
        jql
    };

    let max_results = payload.max_results.unwrap_or(20).min(50);
    let results = client.search_issues(&jql, max_results).await?;
    Ok(ResponseJson(results))
}

// ---------------------------------------------------------------------------
// Import handler
// ---------------------------------------------------------------------------

#[instrument(name = "jira.import_issues", skip(deployment, payload), fields(project_id = %payload.project_id))]
async fn import_issues(
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<JiraImportRequest>,
) -> Result<ResponseJson<ApiResponse<Vec<Issue>>>, ApiError> {
    if payload.issue_keys.is_empty() {
        return Ok(ResponseJson(ApiResponse::success(vec![])));
    }

    if payload.issue_keys.len() > 20 {
        return Err(ApiError::BadRequest(
            "Cannot import more than 20 issues at once".into(),
        ));
    }

    let pool = &deployment.db().pool;

    // Get the default status (first non-hidden status, or create defaults if none exist).
    let statuses = LocalProjectStatus::find_by_project(pool, payload.project_id).await?;
    let default_status_id = if statuses.is_empty() {
        LocalProjectStatus::create_defaults(pool, payload.project_id).await?;
        let new_statuses = LocalProjectStatus::find_by_project(pool, payload.project_id).await?;
        new_statuses
            .first()
            .map(|s| s.id)
            .ok_or_else(|| ApiError::BadRequest("Failed to create default statuses".into()))?
    } else {
        statuses
            .iter()
            .find(|s| !s.hidden)
            .or(statuses.first())
            .map(|s| s.id)
            .unwrap()
    };

    // Use the provided status_id if it's valid, otherwise use default.
    let status_id = payload.status_id;
    let effective_status_id = if statuses.iter().any(|s| s.id == status_id) {
        status_id
    } else {
        default_status_id
    };

    // Check which keys are already imported in this project (check issues table).
    let mut already_imported = std::collections::HashSet::new();
    let existing_issues = LocalIssue::find_by_project(pool, payload.project_id).await?;
    for issue in &existing_issues {
        if let Some(jira_key) = issue
            .extension_metadata
            .get("jira")
            .and_then(|j| j.get("issue_key"))
            .and_then(|v| v.as_str())
        {
            already_imported.insert(jira_key.to_uppercase());
        }
    }

    let keys_to_import: Vec<&String> = payload
        .issue_keys
        .iter()
        .filter(|k| !already_imported.contains(&k.to_uppercase()))
        .collect();

    if keys_to_import.is_empty() {
        return Err(ApiError::Conflict(
            "All selected issues are already imported in this project".into(),
        ));
    }

    let client = build_jira_client(&deployment).await?;
    let mut results = Vec::with_capacity(keys_to_import.len());

    for issue_key in &keys_to_import {
        if !is_issue_key(issue_key) {
            return Err(ApiError::BadRequest(format!(
                "Invalid Jira issue key: {issue_key}"
            )));
        }

        let jira_issue = client.get_issue(issue_key).await.map_err(|e| {
            tracing::error!(?e, %issue_key, "failed to fetch jira issue");
            ApiError::Jira(format!("Failed to fetch Jira issue {issue_key}"))
        })?;

        let priority = jira_issue
            .fields
            .priority
            .as_ref()
            .and_then(|p| map_jira_priority(&p.name));

        let description = jira_issue
            .fields
            .description
            .as_ref()
            .map(|d| adf_to_markdown(d));

        let jira_url = format!("{}/browse/{}", client.site_url(), jira_issue.key);
        let jira_project_key = jira_issue.key.split('-').next().unwrap_or("").to_string();

        // Cache available Jira transitions for status writeback.
        let cached_transitions = match client.get_transitions(&jira_issue.key).await {
            Ok(transitions) => {
                let mut map = serde_json::Map::new();
                for t in &transitions {
                    if let Some(ref cat) = t.to.status_category {
                        map.insert(cat.key.clone(), json!(t.id));
                    }
                }
                json!(map)
            }
            Err(e) => {
                tracing::debug!(?e, issue_key = %jira_issue.key, "could not cache jira transitions");
                json!({})
            }
        };

        let extension_metadata = json!({
            "jira": {
                "issue_key": jira_issue.key,
                "issue_id": jira_issue.id,
                "issue_url": jira_url,
                "jira_project_key": jira_project_key,
                "jira_status": jira_issue.fields.status.as_ref().map(|s| &s.name),
                "imported_at": chrono::Utc::now().to_rfc3339(),
                "transitions": cached_transitions,
            }
        });

        let issue = LocalIssue::create(
            pool,
            &CreateIssueRequest {
                id: None,
                project_id: payload.project_id,
                status_id: effective_status_id,
                title: jira_issue.fields.summary.clone(),
                description,
                priority,
                start_date: None,
                target_date: None,
                completed_at: None,
                sort_order: 0.0,
                parent_issue_id: None,
                parent_issue_sort_order: None,
                extension_metadata,
            },
        )
        .await?;

        results.push(issue);
    }

    Ok(ResponseJson(ApiResponse::success(results)))
}

// ---------------------------------------------------------------------------
// Status sync
// ---------------------------------------------------------------------------

#[instrument(name = "jira.manual_sync_status", skip(deployment), fields(%issue_id))]
async fn manual_sync_status(
    State(deployment): State<DeploymentImpl>,
    Path(issue_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let pool = &deployment.db().pool;

    // Try to find as an Issue first (kanban), then fall back to legacy Task.
    let issue = LocalIssue::find_by_id(pool, issue_id).await?;
    let (extension_metadata, status_name) = if let Some(ref issue) = issue {
        // For issues, look up the status name from project_statuses.
        let status = LocalProjectStatus::find_by_id(pool, issue.status_id).await?;
        let sname = status.map(|s| s.name).unwrap_or_else(|| "todo".into());
        (issue.extension_metadata.clone(), sname)
    } else {
        let task = Task::find_by_id(pool, issue_id)
            .await?
            .ok_or_else(|| ApiError::BadRequest("Issue not found".into()))?;
        (task.extension_metadata.clone(), task.status.to_string())
    };

    if extension_metadata.get("jira").is_none() {
        return Err(ApiError::BadRequest("Issue is not linked to Jira".into()));
    }

    let issue_key = extension_metadata
        .get("jira")
        .and_then(|j| j.get("issue_key"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::BadRequest("Issue has no Jira issue key".into()))?
        .to_string();

    let client = build_jira_client(&deployment).await?;

    // Load custom status mappings.
    let custom_mappings = match StatusMappingRow::find_all(pool).await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(?e, "failed to load custom status mappings, using defaults");
            vec![]
        }
    };
    let custom_match = custom_mappings
        .iter()
        .find(|m| m.vk_status_name.eq_ignore_ascii_case(&status_name))
        .map(|m| m.jira_category_key.as_str());

    let target_category = if let Some(cat) = custom_match {
        cat
    } else {
        match map_status_to_jira_category(&status_name) {
            Some(cat) => cat,
            None => {
                tracing::debug!(%status_name, %issue_key, "no Jira mapping for status");
                return Ok(StatusCode::NO_CONTENT);
            }
        }
    };

    // Try cached transition first, then fall back to live API.
    let jira_meta = &extension_metadata["jira"];
    let cached_transition_id = jira_meta
        .get("transitions")
        .and_then(|t| t.get(target_category))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let (transition_id, transition_name) = if let Some(cached_id) = cached_transition_id {
        (cached_id, format!("cached:{target_category}"))
    } else {
        let transitions = client.get_transitions(&issue_key).await?;
        let matching = transitions.iter().find(|t| {
            t.to.status_category
                .as_ref()
                .is_some_and(|cat| cat.key == target_category)
        });
        match matching {
            Some(t) => (t.id.clone(), t.name.clone()),
            None => {
                tracing::debug!(
                    %issue_key, %target_category,
                    "no matching Jira transition found"
                );
                return Ok(StatusCode::NO_CONTENT);
            }
        }
    };

    // Conflict detection.
    let cached_status = jira_meta
        .get("jira_status")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match client.get_issue(&issue_key).await {
        Err(e) => {
            tracing::warn!(?e, %issue_key, "conflict detection skipped — could not fetch current Jira status");
        }
        Ok(current_jira_issue) => {
            let current_jira_status = current_jira_issue
                .fields
                .status
                .as_ref()
                .map(|s| s.name.as_str())
                .unwrap_or("");

            if !cached_status.is_empty()
                && !current_jira_status.is_empty()
                && cached_status != current_jira_status
            {
                tracing::warn!(
                    %issue_key,
                    cached = %cached_status,
                    current = %current_jira_status,
                    "jira status changed externally, updating cache"
                );
                update_jira_metadata(
                    pool,
                    issue_id,
                    &extension_metadata,
                    &[("jira_status", json!(current_jira_status))],
                    issue.is_some(),
                )
                .await;
                return Ok(StatusCode::NO_CONTENT);
            }
        }
    }

    // Execute the transition.
    match client.transition_issue(&issue_key, &transition_id).await {
        Ok(()) => {
            tracing::info!(%issue_key, transition = %transition_name, "jira status synced");
            let now = chrono::Utc::now().to_rfc3339();
            update_jira_metadata(
                pool,
                issue_id,
                &extension_metadata,
                &[
                    ("last_synced_at", json!(now)),
                    ("jira_status", json!(status_name)),
                ],
                issue.is_some(),
            )
            .await;
        }
        Err(jira_client::JiraError::ApiError {
            status: 400,
            ref message,
        }) if message.contains("field is required") => {
            tracing::info!(
                %issue_key,
                "jira sync skipped — workflow requires mandatory fields"
            );
        }
        Err(e) => {
            tracing::warn!(?e, %issue_key, "jira transition failed");
            return Err(ApiError::Jira(format!("Failed to sync status: {e}")));
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Comments
// ---------------------------------------------------------------------------

#[instrument(name = "jira.get_comments", skip(deployment), fields(%issue_id))]
async fn get_jira_comments(
    State(deployment): State<DeploymentImpl>,
    Path(issue_id): Path<Uuid>,
) -> Result<ResponseJson<Vec<JiraCommentView>>, ApiError> {
    let pool = &deployment.db().pool;
    let ext_meta = if let Some(issue) = LocalIssue::find_by_id(pool, issue_id).await? {
        issue.extension_metadata
    } else if let Some(task) = Task::find_by_id(pool, issue_id).await? {
        task.extension_metadata
    } else {
        return Err(ApiError::BadRequest("Issue not found".into()));
    };

    let jira_key = ext_meta
        .get("jira")
        .and_then(|j| j.get("issue_key"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::BadRequest("Issue has no Jira metadata".into()))?
        .to_string();

    let client = build_jira_client(&deployment).await?;
    let response = client.get_comments(&jira_key).await?;

    let views: Vec<JiraCommentView> = response
        .comments
        .into_iter()
        .map(|c| {
            let body_md = c
                .body
                .as_ref()
                .map(|b| adf_to_markdown(b))
                .unwrap_or_default();
            let author = c
                .author
                .as_ref()
                .map(|a| a.display_name.clone())
                .unwrap_or_else(|| "Unknown".to_string());
            JiraCommentView {
                id: c.id,
                author_name: author,
                body_markdown: body_md,
                created: c.created,
            }
        })
        .collect();

    Ok(ResponseJson(views))
}

#[instrument(name = "jira.post_summary", skip(deployment), fields(%issue_id))]
async fn post_summary_to_jira(
    State(deployment): State<DeploymentImpl>,
    Path(issue_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let pool = &deployment.db().pool;
    let (ext_meta, title, description) =
        if let Some(issue) = LocalIssue::find_by_id(pool, issue_id).await? {
            (issue.extension_metadata, issue.title, issue.description)
        } else if let Some(task) = Task::find_by_id(pool, issue_id).await? {
            (task.extension_metadata, task.title, task.description)
        } else {
            return Err(ApiError::BadRequest("Issue not found".into()));
        };

    let jira_key = ext_meta
        .get("jira")
        .and_then(|j| j.get("issue_key"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::BadRequest("Issue has no Jira metadata".into()))?
        .to_string();

    let mut summary = format!("[Posted via Vibe Kanban]\n\n{}", title);

    if let Some(desc) = &description {
        if !desc.is_empty() {
            let truncated = if desc.len() > 500 {
                let end = desc
                    .char_indices()
                    .take_while(|(i, _)| *i < 500)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(500.min(desc.len()));
                format!("{}...", &desc[..end])
            } else {
                desc.clone()
            };
            summary.push_str(&format!("\n\n{truncated}"));
        }
    }

    let client = build_jira_client(&deployment).await?;
    client.add_comment(&jira_key, &summary).await?;

    tracing::info!(%jira_key, "posted summary to Jira");
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Status mappings & statuses
// ---------------------------------------------------------------------------

#[instrument(name = "jira.get_statuses", skip(deployment))]
async fn get_jira_statuses(
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<Vec<JiraStatusView>>, ApiError> {
    let client = build_jira_client(&deployment).await?;
    let statuses = client.get_statuses().await?;

    let mut views: Vec<JiraStatusView> = statuses
        .into_iter()
        .filter_map(|s| {
            let cat = s.status_category?;
            Some(JiraStatusView {
                name: s.name,
                category_key: cat.key,
                category_name: cat.name,
            })
        })
        .collect();

    views.sort_by(|a, b| a.name.cmp(&b.name));
    views.dedup_by(|a, b| a.name == b.name);

    Ok(ResponseJson(views))
}

async fn get_status_mappings(
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<Vec<JiraStatusMapping>>, ApiError> {
    let rows = StatusMappingRow::find_all(&deployment.db().pool).await?;
    let mappings = rows
        .into_iter()
        .map(|r| JiraStatusMapping {
            vk_status_name: r.vk_status_name,
            jira_category_key: r.jira_category_key,
        })
        .collect();
    Ok(ResponseJson(mappings))
}

#[instrument(name = "jira.upsert_status_mapping", skip(deployment, payload))]
async fn upsert_status_mapping(
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<JiraStatusMappingRequest>,
) -> Result<StatusCode, ApiError> {
    StatusMappingRow::upsert(
        &deployment.db().pool,
        &payload.vk_status_name,
        &payload.jira_category_key,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[instrument(name = "jira.delete_status_mapping", skip(deployment, payload))]
async fn delete_status_mapping(
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<JiraStatusMappingDeleteRequest>,
) -> Result<StatusCode, ApiError> {
    StatusMappingRow::delete(&deployment.db().pool, &payload.vk_status_name).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// OAuth stub (not supported in local mode v1)
// ---------------------------------------------------------------------------

async fn oauth_authorize_stub() -> Result<ResponseJson<JiraOAuthAuthorizeResponse>, ApiError> {
    Err(ApiError::BadRequest(
        "OAuth is not supported in local mode. Use API token authentication.".into(),
    ))
}
