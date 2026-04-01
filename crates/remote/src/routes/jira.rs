use api_types::{CreateIssueRequest, Issue, IssuePriority, MutationResponse};
use axum::{
    Json, Router,
    extract::{Extension, Path, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use jira_client::{
    JiraAuth, JiraClient, JiraCommentView, JiraConnectRequest, JiraConnectionInfo,
    JiraImportRequest, JiraOAuthAuthorizeResponse, JiraSearchRequest, JiraSearchResult,
    JiraStatusMapping, JiraStatusMappingDeleteRequest, JiraStatusMappingRequest, JiraStatusView,
    adf_to_markdown, exchange_oauth_code, get_accessible_resources, refresh_oauth_token,
    types::JiraProject,
};
use secrecy::ExposeSecret;
use serde_json::json;
use tracing::instrument;
use uuid::Uuid;

use super::{error::ErrorResponse, organization_members::ensure_member_access};
use crate::{
    AppState,
    auth::RequestContext,
    db::{
        issue_tags::IssueTagRepository, issues::IssueRepository, jira::JiraRepository,
        projects::ProjectRepository, tags::TagRepository,
    },
};

pub(super) fn router() -> Router<AppState> {
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
        .route("/jira/oauth/authorize", get(oauth_authorize))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the org_id from the user's first org membership.
/// Jira connections are org-scoped, so the user must be a member.
async fn resolve_org_id(
    state: &AppState,
    user_id: Uuid,
    organization_id: Uuid,
) -> Result<(), ErrorResponse> {
    ensure_member_access(state.pool(), organization_id, user_id).await
}

/// Build a JiraClient from the stored connection for an org.
/// Automatically refreshes expired OAuth2 tokens and persists the new credentials.
async fn build_jira_client(
    state: &AppState,
    organization_id: Uuid,
) -> Result<JiraClient, ErrorResponse> {
    let row = JiraRepository::find_by_org(state.pool(), organization_id)
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to load jira connection");
            ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?
        .ok_or_else(|| {
            ErrorResponse::new(StatusCode::NOT_FOUND, "no Jira connection configured")
        })?;

    let credentials_json = state
        .jwt()
        .decrypt_provider_tokens_raw(&row.encrypted_credentials)
        .map_err(|e| {
            tracing::error!(?e, "failed to decrypt jira credentials");
            ErrorResponse::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Jira credentials are invalid — please reconnect Jira in Settings",
            )
        })?;

    let mut auth: JiraAuth = serde_json::from_str(&credentials_json).map_err(|e| {
        tracing::error!(?e, "failed to parse jira credentials");
        ErrorResponse::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Jira credentials are invalid — please reconnect Jira in Settings",
        )
    })?;

    // Refresh expired OAuth2 tokens transparently.
    if auth.is_expired() {
        auth = maybe_refresh_oauth_token(state, organization_id, &auth)
            .await
            .map_err(|e| {
                tracing::warn!(?e, "jira OAuth token refresh failed");
                ErrorResponse::new(
                    StatusCode::UNAUTHORIZED,
                    "Jira OAuth token expired and refresh failed — please reconnect",
                )
            })?;
    }

    Ok(JiraClient::new(&row.jira_site_url, auth))
}

/// Refresh an OAuth2 token and persist the new credentials to the DB.
/// Returns the original auth unchanged if it's not an OAuth2 token.
///
/// Uses a PostgreSQL advisory lock (keyed by organization_id) to prevent
/// concurrent refresh races. Atlassian uses refresh-token rotation, so two
/// simultaneous refreshes with the same token will desync the stored credentials.
/// After acquiring the lock we re-read from the DB; if another request already
/// refreshed the token we return the fresh copy without hitting the Atlassian API.
async fn maybe_refresh_oauth_token(
    state: &AppState,
    organization_id: Uuid,
    auth: &JiraAuth,
) -> Result<JiraAuth, jira_client::JiraError> {
    if !matches!(auth, JiraAuth::OAuth2 { .. }) {
        return Ok(auth.clone());
    }

    let oauth_config = state
        .config
        .jira_oauth
        .as_ref()
        .ok_or(jira_client::JiraError::NotConfigured)?;

    // Derive a stable i64 lock key from the first 8 bytes of the org UUID.
    let lock_key = i64::from_ne_bytes(
        organization_id.as_bytes()[..8]
            .try_into()
            .expect("UUID has ≥8 bytes"),
    );

    // Use a transaction so the advisory lock is automatically released when the
    // transaction ends — even on early returns or errors.
    let mut tx = state
        .pool()
        .begin()
        .await
        .map_err(|e| jira_client::JiraError::Parse(format!("begin tx: {e}")))?;

    // Acquire a transaction-scoped advisory lock; auto-released on commit/rollback.
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(|e| jira_client::JiraError::Parse(format!("advisory lock: {e}")))?;

    // Re-read credentials from DB — another concurrent request may have already
    // refreshed the token while we were waiting for the lock.
    let fresh_auth: JiraAuth =
        match JiraRepository::find_by_org(state.pool(), organization_id).await {
            Ok(Some(row)) => {
                match state
                    .jwt()
                    .decrypt_provider_tokens_raw(&row.encrypted_credentials)
                {
                    Ok(json) => match serde_json::from_str(&json) {
                        Ok(a) => a,
                        Err(e) => {
                            tracing::warn!(
                                ?e,
                                "failed to parse re-read jira credentials during refresh"
                            );
                            auth.clone()
                        }
                    },
                    Err(e) => {
                        tracing::warn!(
                            ?e,
                            "failed to decrypt re-read jira credentials during refresh"
                        );
                        auth.clone()
                    }
                }
            }
            Ok(None) => auth.clone(),
            Err(e) => {
                tracing::warn!(?e, "failed to re-read jira connection during token refresh");
                auth.clone()
            }
        };

    // If another request already refreshed, return the fresh token immediately.
    if !fresh_auth.is_expired() {
        let _ = tx.commit().await;
        return Ok(fresh_auth);
    }

    let refresh_tok = match &fresh_auth {
        JiraAuth::OAuth2 { refresh_token, .. } => refresh_token.clone(),
        _ => {
            let _ = tx.commit().await;
            return Ok(fresh_auth);
        }
    };

    tracing::info!(organization_id = %organization_id, "refreshing expired Jira OAuth token");

    let refreshed = match refresh_oauth_token(
        oauth_config.client_id(),
        oauth_config.client_secret().expose_secret(),
        &refresh_tok,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            // tx drops here → lock auto-released
            return Err(e);
        }
    };

    // Persist the refreshed credentials.
    let auth_json = serde_json::to_string(&refreshed)
        .map_err(|e| jira_client::JiraError::Parse(format!("serialize: {e}")))?;
    let encrypted = state
        .jwt()
        .encrypt_provider_tokens_raw(auth_json.as_bytes())
        .map_err(|e| jira_client::JiraError::Parse(format!("encrypt: {e}")))?;

    if let Err(e) =
        JiraRepository::update_credentials(state.pool(), organization_id, &encrypted).await
    {
        tracing::warn!(?e, "failed to persist refreshed Jira OAuth tokens");
        // Non-fatal — the refreshed token still works for this request.
    }

    let _ = tx.commit().await;
    Ok(refreshed)
}

// ---------------------------------------------------------------------------
// Jira status writeback (called from issues.rs and pull_requests.rs)
// ---------------------------------------------------------------------------

/// Sync a VK issue's status change to Jira. Non-blocking — logs warnings on failure.
/// Call this after an issue's status_id changes if the issue has Jira metadata.
pub(crate) async fn sync_jira_status_if_linked(
    state: &AppState,
    issue: &Issue,
    new_status_name: &str,
) {
    // Check if this issue has Jira metadata.
    let jira_meta = match issue.extension_metadata.get("jira") {
        Some(meta) => meta,
        None => return, // Not a Jira-linked issue.
    };

    let issue_key = match jira_meta.get("issue_key").and_then(|v| v.as_str()) {
        Some(key) => key.to_string(),
        None => return,
    };

    // Resolve organization_id from the project.
    let organization_id =
        match ProjectRepository::organization_id(state.pool(), issue.project_id).await {
            Ok(Some(org_id)) => org_id,
            Ok(None) => {
                tracing::warn!(%issue_key, "jira writeback: project has no organization");
                return;
            }
            Err(e) => {
                tracing::warn!(?e, %issue_key, "jira writeback: failed to resolve org_id");
                return;
            }
        };

    // Build Jira client.
    let client = match build_jira_client_for_sync(state, organization_id).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(?e, %issue_key, "jira writeback: no jira connection");
            return;
        }
    };

    // Map VK status name → Jira status category key.
    // Check custom mappings first, then fall back to hardcoded defaults.
    let custom_mappings = match JiraRepository::get_status_mappings(state.pool(), organization_id)
        .await
    {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(?e, %issue_key, "jira writeback: failed to load custom mappings, using defaults");
            vec![]
        }
    };

    let custom_match = custom_mappings
        .iter()
        .find(|m| m.vk_status_name.eq_ignore_ascii_case(new_status_name))
        .map(|m| m.jira_category_key.as_str());

    let target_category = if let Some(cat) = custom_match {
        cat
    } else {
        match new_status_name.to_lowercase().as_str() {
            "done" | "cancelled" | "canceled" => "done",
            "in progress" | "in_progress" => "indeterminate",
            "in review" | "in_review" => "indeterminate",
            "to do" | "todo" | "backlog" => "new",
            _ => {
                tracing::debug!(%new_status_name, %issue_key, "jira writeback: no mapping for status");
                return;
            }
        }
    };

    // Try cached transition first, then fall back to live API.
    let cached_transition_id = jira_meta
        .get("transitions")
        .and_then(|t| t.get(target_category))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let (transition_id, transition_name) = if let Some(cached_id) = cached_transition_id {
        (cached_id, format!("cached:{target_category}"))
    } else {
        // Fall back to live transition lookup.
        let transitions = match client.get_transitions(&issue_key).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(?e, %issue_key, "jira writeback: failed to get transitions");
                return;
            }
        };

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
                    available = ?transitions.iter().map(|t| &t.name).collect::<Vec<_>>(),
                    "jira writeback: no matching transition found"
                );
                return;
            }
        }
    };

    // Conflict detection: check if Jira status changed independently.
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
                    "jira writeback: conflict — Jira status changed externally, updating cache"
                );
                // Update cached status to current Jira value, skip transition.
                update_jira_metadata_field(state, issue, "jira_status", json!(current_jira_status))
                    .await;
                return;
            }
        }
    }

    // Execute the transition.
    match client.transition_issue(&issue_key, &transition_id).await {
        Ok(()) => {
            tracing::info!(%issue_key, transition = %transition_name, "jira writeback: transitioned successfully");
            // Update last_synced_at and jira_status after successful sync.
            let now = chrono::Utc::now().to_rfc3339();
            update_jira_metadata_fields(
                state,
                issue,
                &[
                    ("last_synced_at", json!(now)),
                    ("jira_status", json!(new_status_name)),
                ],
            )
            .await;
        }
        Err(jira_client::JiraError::ApiError {
            status: 400,
            ref message,
        }) if message.contains("field is required") => {
            tracing::info!(
                %issue_key, transition = %transition_name,
                "jira writeback: skipped — Jira workflow requires mandatory fields that cannot be set automatically"
            );
        }
        Err(e) => {
            tracing::warn!(?e, %issue_key, transition = %transition_name, "jira writeback: transition failed");
        }
    }
}

/// Update a single field in `extension_metadata.jira` for an issue.
async fn update_jira_metadata_field(
    state: &AppState,
    issue: &Issue,
    field: &str,
    value: serde_json::Value,
) {
    update_jira_metadata_fields(state, issue, &[(field, value)]).await;
}

/// Update multiple fields in `extension_metadata.jira` for an issue.
async fn update_jira_metadata_fields(
    state: &AppState,
    issue: &Issue,
    fields: &[(&str, serde_json::Value)],
) {
    let mut metadata = issue.extension_metadata.clone();
    if let Some(jira) = metadata.get_mut("jira").and_then(|v| v.as_object_mut()) {
        for (key, value) in fields {
            jira.insert((*key).to_string(), value.clone());
        }
    }

    if let Err(e) =
        sqlx::query("UPDATE issues SET extension_metadata = $1, updated_at = NOW() WHERE id = $2")
            .bind(&metadata)
            .bind(issue.id)
            .execute(state.pool())
            .await
    {
        tracing::warn!(?e, issue_id = %issue.id, "failed to update jira metadata");
    }
}

/// Build a JiraClient without returning ErrorResponse (for background use).
async fn build_jira_client_for_sync(
    state: &AppState,
    organization_id: Uuid,
) -> Result<JiraClient, String> {
    let row = JiraRepository::find_by_org(state.pool(), organization_id)
        .await
        .map_err(|e| format!("db error: {e}"))?
        .ok_or_else(|| "no jira connection".to_string())?;

    let credentials_json = state
        .jwt()
        .decrypt_provider_tokens_raw(&row.encrypted_credentials)
        .map_err(|e| format!("decrypt error: {e}"))?;

    let mut auth: JiraAuth =
        serde_json::from_str(&credentials_json).map_err(|e| format!("parse error: {e}"))?;

    // Refresh expired OAuth2 tokens.
    if auth.is_expired() {
        auth = maybe_refresh_oauth_token(state, organization_id, &auth)
            .await
            .map_err(|e| format!("oauth refresh: {e}"))?;
    }

    Ok(JiraClient::new(&row.jira_site_url, auth))
}

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
struct OrgQuery {
    organization_id: Uuid,
}

#[instrument(name = "jira.get_connection", skip(state, ctx))]
async fn get_connection(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    axum::extract::Query(query): axum::extract::Query<OrgQuery>,
) -> Result<Json<JiraConnectionInfo>, ErrorResponse> {
    resolve_org_id(&state, ctx.user.id, query.organization_id).await?;

    let row = JiraRepository::find_by_org(state.pool(), query.organization_id)
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to load jira connection");
            ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?;

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

    Ok(Json(info))
}

#[instrument(name = "jira.connect", skip(state, ctx, payload))]
async fn connect(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    axum::extract::Query(query): axum::extract::Query<OrgQuery>,
    Json(payload): Json<JiraConnectRequest>,
) -> Result<Json<JiraConnectionInfo>, ErrorResponse> {
    resolve_org_id(&state, ctx.user.id, query.organization_id).await?;

    let mut site_url = payload.site_url.trim().trim_end_matches('/').to_string();

    // For API token auth, site_url is required upfront.
    // For OAuth, it can be "auto" and will be resolved from accessible resources.
    let is_oauth = matches!(payload.auth, jira_client::JiraAuthPayload::OAuth2 { .. });
    let auto_detect_site = is_oauth && (site_url.is_empty() || site_url == "auto");

    if !auto_detect_site {
        if let Err(msg) = jira_client::validate_jira_site_url(&site_url) {
            return Err(ErrorResponse::new(StatusCode::BAD_REQUEST, msg));
        }
    }

    // Build the JiraAuth from the payload and verify the connection works.
    let (auth, auth_type) = match payload.auth {
        jira_client::JiraAuthPayload::ApiToken { email, token } => {
            let auth = JiraAuth::ApiToken {
                email: email.clone(),
                token: token.clone(),
            };
            (auth, "api_token")
        }
        jira_client::JiraAuthPayload::OAuth2 { code, redirect_uri } => {
            let oauth_config = state.config.jira_oauth.as_ref().ok_or_else(|| {
                ErrorResponse::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Jira OAuth is not configured on this server",
                )
            })?;

            let auth = exchange_oauth_code(
                oauth_config.client_id(),
                oauth_config.client_secret().expose_secret(),
                &code,
                &redirect_uri,
            )
            .await
            .map_err(|e| {
                tracing::warn!(?e, "jira OAuth code exchange failed");
                ErrorResponse::new(
                    StatusCode::BAD_REQUEST,
                    format!("OAuth exchange failed: {e}"),
                )
            })?;

            (auth, "oauth2")
        }
    };

    // For OAuth: auto-detect site URL from accessible resources if needed.
    if auto_detect_site {
        let access_token = match &auth {
            JiraAuth::OAuth2 { access_token, .. } => access_token.clone(),
            _ => unreachable!(),
        };
        let sites = get_accessible_resources(&access_token).await.map_err(|e| {
            tracing::warn!(?e, "failed to fetch accessible Atlassian sites");
            ErrorResponse::new(
                StatusCode::BAD_REQUEST,
                "Failed to fetch your Atlassian sites — check your OAuth permissions",
            )
        })?;
        let first_site = sites.into_iter().next().ok_or_else(|| {
            ErrorResponse::new(
                StatusCode::BAD_REQUEST,
                "No accessible Atlassian sites found for this account",
            )
        })?;
        site_url = first_site.url.trim_end_matches('/').to_string();
        tracing::info!(site_url = %site_url, "auto-detected Jira site URL from OAuth");
    }

    // Verify the credentials work by fetching projects.
    let client = JiraClient::new(&site_url, auth.clone());
    client.get_projects().await.map_err(|e| {
        tracing::warn!(?e, "jira credential verification failed");
        ErrorResponse::new(
            StatusCode::BAD_REQUEST,
            "Failed to connect to Jira — check your credentials and site URL",
        )
    })?;

    // Encrypt and store.
    let auth_json = serde_json::to_string(&auth).map_err(|_| {
        ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "serialization error")
    })?;

    let encrypted = state
        .jwt()
        .encrypt_provider_tokens_raw(auth_json.as_bytes())
        .map_err(|e| {
            tracing::error!(?e, "failed to encrypt jira credentials");
            ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "encryption error")
        })?;

    let row = JiraRepository::upsert(
        state.pool(),
        query.organization_id,
        &site_url,
        auth_type,
        &encrypted,
        ctx.user.id,
    )
    .await
    .map_err(|e| {
        tracing::error!(?e, "failed to save jira connection");
        ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "database error")
    })?;

    Ok(Json(JiraConnectionInfo {
        connected: true,
        site_url: Some(row.jira_site_url),
        auth_type: Some(row.auth_type),
        connected_at: Some(row.created_at.to_rfc3339()),
    }))
}

#[instrument(name = "jira.disconnect", skip(state, ctx))]
async fn disconnect(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    axum::extract::Query(query): axum::extract::Query<OrgQuery>,
) -> Result<StatusCode, ErrorResponse> {
    resolve_org_id(&state, ctx.user.id, query.organization_id).await?;

    JiraRepository::delete_by_org(state.pool(), query.organization_id)
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to delete jira connection");
            ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?;

    Ok(StatusCode::NO_CONTENT)
}

#[instrument(name = "jira.list_projects", skip(state, ctx))]
async fn list_projects(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    axum::extract::Query(query): axum::extract::Query<OrgQuery>,
) -> Result<Json<Vec<JiraProject>>, ErrorResponse> {
    resolve_org_id(&state, ctx.user.id, query.organization_id).await?;

    let client = build_jira_client(&state, query.organization_id).await?;
    let projects = client.get_projects().await.map_err(|e| {
        tracing::error!(?e, "failed to list jira projects");
        ErrorResponse::new(StatusCode::BAD_GATEWAY, "failed to fetch Jira projects")
    })?;

    Ok(Json(projects))
}

#[instrument(name = "jira.search_issues", skip(state, ctx, payload))]
async fn search_issues(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    axum::extract::Query(query): axum::extract::Query<OrgQuery>,
    Json(payload): Json<JiraSearchRequest>,
) -> Result<Json<JiraSearchResult>, ErrorResponse> {
    resolve_org_id(&state, ctx.user.id, query.organization_id).await?;

    let client = build_jira_client(&state, query.organization_id).await?;

    // Build JQL from the search request.
    let trimmed = payload.query.trim();
    let jql = if trimmed.contains('=') || trimmed.contains("ORDER BY") {
        // User provided raw JQL.
        trimmed.to_string()
    } else if is_issue_key(trimmed) {
        // Looks like an issue key (e.g. "DEV-170227") — search by key.
        format!("key = \"{trimmed}\"")
    } else {
        // Text search with optional project filter.
        let escaped = trimmed.replace('"', "\\\"");
        let mut jql = format!("text ~ \"{escaped}\"");
        if let Some(ref project_key) = payload.project_key {
            // Validate project key format to prevent JQL injection.
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
    let results = client.search_issues(&jql, max_results).await.map_err(|e| {
        tracing::error!(?e, "jira search failed");
        ErrorResponse::new(StatusCode::BAD_GATEWAY, "Jira search failed")
    })?;

    Ok(Json(results))
}

#[instrument(name = "jira.import_issues", skip(state, ctx, payload), fields(project_id = %payload.project_id))]
async fn import_issues(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    axum::extract::Query(query): axum::extract::Query<OrgQuery>,
    Json(payload): Json<JiraImportRequest>,
) -> Result<Json<Vec<MutationResponse<Issue>>>, ErrorResponse> {
    resolve_org_id(&state, ctx.user.id, query.organization_id).await?;

    if payload.issue_keys.is_empty() {
        return Ok(Json(vec![]));
    }

    if payload.issue_keys.len() > 20 {
        return Err(ErrorResponse::new(
            StatusCode::BAD_REQUEST,
            "Cannot import more than 20 issues at once",
        ));
    }

    // Check which keys are already imported in this project.
    let already_imported =
        find_existing_jira_keys(state.pool(), payload.project_id, &payload.issue_keys)
            .await
            .map_err(|e| {
                tracing::error!(?e, "failed to check existing jira imports");
                ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "database error")
            })?;

    let keys_to_import: Vec<&String> = payload
        .issue_keys
        .iter()
        .filter(|k| !already_imported.contains(&k.as_str().to_uppercase()))
        .collect();

    if keys_to_import.is_empty() {
        return Err(ErrorResponse::new(
            StatusCode::CONFLICT,
            "All selected issues are already imported in this project",
        ));
    }

    let client = build_jira_client(&state, query.organization_id).await?;
    let mut results = Vec::with_capacity(keys_to_import.len());

    for issue_key in &keys_to_import {
        if !is_issue_key(issue_key) {
            return Err(ErrorResponse::new(
                StatusCode::BAD_REQUEST,
                format!("invalid Jira issue key: {issue_key}"),
            ));
        }

        let jira_issue = client.get_issue(issue_key).await.map_err(|e| {
            tracing::error!(?e, %issue_key, "failed to fetch jira issue");
            ErrorResponse::new(
                StatusCode::BAD_GATEWAY,
                format!("failed to fetch Jira issue {issue_key}"),
            )
        })?;

        // Map Jira priority to VK priority.
        let priority = jira_issue.fields.priority.as_ref().and_then(|p| {
            match p.name.to_lowercase().as_str() {
                "highest" | "blocker" => Some(IssuePriority::Urgent),
                "high" | "critical" => Some(IssuePriority::High),
                "medium" => Some(IssuePriority::Medium),
                "low" | "lowest" | "trivial" => Some(IssuePriority::Low),
                _ => None,
            }
        });

        // Convert ADF description to markdown.
        let description = jira_issue
            .fields
            .description
            .as_ref()
            .map(|d| adf_to_markdown(d));

        // Build extension_metadata with Jira link info.
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

        let create_result = IssueRepository::create(
            state.pool(),
            None,
            payload.project_id,
            payload.status_id,
            jira_issue.fields.summary.clone(),
            description,
            priority,
            None,
            None,
            None,
            0.0, // Will be sorted to top (lowest sort_order).
            None,
            None,
            extension_metadata,
            ctx.user.id,
        )
        .await;

        let response = match create_result {
            Ok(r) => r,
            Err(crate::db::issues::IssueError::Database(sqlx::Error::Database(ref db_err)))
                if db_err.code().as_deref() == Some("23505") =>
            {
                // Unique constraint violation — another request already imported this key
                // (race between application-level check and DB index). Skip silently.
                tracing::debug!(%issue_key, "skipping duplicate jira import (unique constraint)");
                continue;
            }
            Err(e) => {
                tracing::error!(?e, %issue_key, "failed to create VK issue from jira");
                return Err(ErrorResponse::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("failed to import {issue_key}"),
                ));
            }
        };

        // Map Jira labels → VK tags.
        let created_issue_id = response.data.id;
        for label in &jira_issue.fields.labels {
            if label.trim().is_empty() {
                continue;
            }
            match TagRepository::find_or_create_by_name(state.pool(), payload.project_id, label)
                .await
            {
                Ok(tag) => {
                    if let Err(e) =
                        IssueTagRepository::create(state.pool(), None, created_issue_id, tag.id)
                            .await
                    {
                        tracing::warn!(?e, %label, "failed to link jira label as tag");
                    }
                }
                Err(e) => {
                    tracing::warn!(?e, %label, "failed to find or create tag for jira label");
                }
            }
        }

        results.push(response);
    }

    Ok(Json(results))
}

/// Manually trigger a Jira status sync for a single issue.
#[instrument(name = "jira.manual_sync_status", skip(state, ctx), fields(%issue_id))]
async fn manual_sync_status(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Path(issue_id): Path<Uuid>,
) -> Result<StatusCode, ErrorResponse> {
    let issue = IssueRepository::find_by_id(state.pool(), issue_id)
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to load issue");
            ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?
        .ok_or_else(|| ErrorResponse::new(StatusCode::NOT_FOUND, "issue not found"))?;

    // Authorize first — prevent information leakage about Jira linkage.
    let organization_id = super::organization_members::ensure_project_access(
        state.pool(),
        ctx.user.id,
        issue.project_id,
    )
    .await?;

    if issue.extension_metadata.get("jira").is_none() {
        return Err(ErrorResponse::new(
            StatusCode::BAD_REQUEST,
            "Issue is not linked to Jira",
        ));
    }

    let status = crate::db::project_statuses::ProjectStatusRepository::find_by_id(
        state.pool(),
        issue.status_id,
    )
    .await
    .map_err(|_| ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "failed to load status"))?
    .ok_or_else(|| ErrorResponse::new(StatusCode::NOT_FOUND, "status not found"))?;

    // Run sync synchronously so caller gets immediate feedback.
    sync_jira_status_if_linked(&state, &issue, &status.name).await;

    Ok(StatusCode::NO_CONTENT)
}

async fn find_existing_jira_keys(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    issue_keys: &[String],
) -> Result<std::collections::HashSet<String>, sqlx::Error> {
    let keys: Vec<&str> = issue_keys.iter().map(|k| k.as_str()).collect();
    let rows = sqlx::query_scalar::<_, String>(
        r#"
        SELECT UPPER(extension_metadata->'jira'->>'issue_key')
        FROM issues
        WHERE project_id = $1
          AND extension_metadata->'jira'->>'issue_key' IS NOT NULL
          AND UPPER(extension_metadata->'jira'->>'issue_key') = ANY(
              SELECT UPPER(unnest($2::text[]))
          )
        "#,
    )
    .bind(project_id)
    .bind(&keys)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Check if a string looks like a Jira issue key (e.g. `DEV-170227`, `PROJ-1`).
fn is_issue_key(s: &str) -> bool {
    let Some((prefix, number)) = s.split_once('-') else {
        return false;
    };
    !prefix.is_empty()
        && prefix.chars().all(|c| c.is_ascii_uppercase())
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

// ---------------------------------------------------------------------------
// OAuth authorize URL
// ---------------------------------------------------------------------------

const ATLASSIAN_AUTHORIZE_URL: &str = "https://auth.atlassian.com/authorize";
const JIRA_OAUTH_SCOPES: &str = "read:jira-work write:jira-work read:jira-user offline_access";

#[instrument(name = "jira.oauth_authorize", skip(state, ctx))]
async fn oauth_authorize(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    axum::extract::Query(query): axum::extract::Query<OrgQuery>,
) -> Result<Json<JiraOAuthAuthorizeResponse>, ErrorResponse> {
    resolve_org_id(&state, ctx.user.id, query.organization_id).await?;

    let oauth_config = state.config.jira_oauth.as_ref().ok_or_else(|| {
        ErrorResponse::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Jira OAuth is not configured on this server",
        )
    })?;

    let base_url = state
        .config
        .server_public_base_url
        .as_deref()
        .unwrap_or("http://localhost:8081");

    let redirect_uri = format!("{base_url}/jira/oauth/callback");

    // Build CSRF-safe state: org_id + HMAC signature using OAuth client secret.
    let org_id_str = query.organization_id.to_string();
    let state_param = {
        use base64::Engine;
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;
        let mut mac =
            HmacSha256::new_from_slice(oauth_config.client_secret().expose_secret().as_bytes())
                .expect("HMAC accepts any key length");
        mac.update(org_id_str.as_bytes());
        let sig =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        format!("{org_id_str}:{sig}")
    };

    let authorize_url = format!(
        "{ATLASSIAN_AUTHORIZE_URL}?audience=api.atlassian.com\
        &client_id={client_id}\
        &scope={scopes}\
        &redirect_uri={redirect_uri}\
        &state={state_param}\
        &response_type=code\
        &prompt=consent",
        client_id = urlencoding::encode(oauth_config.client_id()),
        scopes = urlencoding::encode(JIRA_OAUTH_SCOPES),
        redirect_uri = urlencoding::encode(&redirect_uri),
        state_param = urlencoding::encode(&state_param),
    );

    Ok(Json(JiraOAuthAuthorizeResponse { authorize_url }))
}

// ---------------------------------------------------------------------------
// Comments
// ---------------------------------------------------------------------------

#[instrument(name = "jira.get_comments", skip(state, ctx), fields(%issue_id))]
async fn get_jira_comments(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Path(issue_id): Path<Uuid>,
) -> Result<Json<Vec<JiraCommentView>>, ErrorResponse> {
    let issue = IssueRepository::find_by_id(state.pool(), issue_id)
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to load issue");
            ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?
        .ok_or_else(|| ErrorResponse::new(StatusCode::NOT_FOUND, "issue not found"))?;

    let jira_key = issue
        .extension_metadata
        .get("jira")
        .and_then(|j| j.get("issue_key"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| ErrorResponse::new(StatusCode::BAD_REQUEST, "issue has no Jira metadata"))?
        .to_string();

    let organization_id = ProjectRepository::organization_id(state.pool(), issue.project_id)
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to resolve org");
            ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?
        .ok_or_else(|| ErrorResponse::new(StatusCode::NOT_FOUND, "project not found"))?;

    resolve_org_id(&state, ctx.user.id, organization_id).await?;

    let client = build_jira_client(&state, organization_id).await?;
    let response = client.get_comments(&jira_key).await.map_err(|e| {
        tracing::warn!(?e, "failed to fetch Jira comments");
        ErrorResponse::new(StatusCode::BAD_GATEWAY, "failed to fetch Jira comments")
    })?;

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

    Ok(Json(views))
}

#[instrument(name = "jira.post_summary", skip(state, ctx), fields(%issue_id))]
async fn post_summary_to_jira(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Path(issue_id): Path<Uuid>,
) -> Result<StatusCode, ErrorResponse> {
    let issue = IssueRepository::find_by_id(state.pool(), issue_id)
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to load issue");
            ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?
        .ok_or_else(|| ErrorResponse::new(StatusCode::NOT_FOUND, "issue not found"))?;

    let jira_key = issue
        .extension_metadata
        .get("jira")
        .and_then(|j| j.get("issue_key"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| ErrorResponse::new(StatusCode::BAD_REQUEST, "issue has no Jira metadata"))?
        .to_string();

    let organization_id = ProjectRepository::organization_id(state.pool(), issue.project_id)
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to resolve org");
            ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?
        .ok_or_else(|| ErrorResponse::new(StatusCode::NOT_FOUND, "project not found"))?;

    resolve_org_id(&state, ctx.user.id, organization_id).await?;

    // Build summary text from issue data + PR links.
    let mut summary = format!("[Posted via Vibe Kanban]\n\n{}", issue.title);

    if let Some(desc) = &issue.description {
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

    // Add PR links if available.
    let pr_urls: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT pr.url FROM pull_requests pr
        JOIN pull_request_issues pri ON pri.pull_request_id = pr.id
        WHERE pri.issue_id = $1
        "#,
    )
    .bind(issue_id)
    .fetch_all(state.pool())
    .await
    .unwrap_or_else(|e| {
        tracing::warn!(?e, %issue_id, "failed to fetch PR URLs for Jira summary");
        vec![]
    });

    for url in &pr_urls {
        summary.push_str(&format!("\nPR: {url}"));
    }

    let client = build_jira_client(&state, organization_id).await?;
    client.add_comment(&jira_key, &summary).await.map_err(|e| {
        tracing::warn!(?e, "failed to post summary to Jira");
        ErrorResponse::new(StatusCode::BAD_GATEWAY, "failed to post summary to Jira")
    })?;

    tracing::info!(%jira_key, "posted summary to Jira");
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Status mappings
// ---------------------------------------------------------------------------

#[instrument(name = "jira.get_statuses", skip(state, ctx))]
async fn get_jira_statuses(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    axum::extract::Query(query): axum::extract::Query<OrgQuery>,
) -> Result<Json<Vec<JiraStatusView>>, ErrorResponse> {
    resolve_org_id(&state, ctx.user.id, query.organization_id).await?;

    let client = build_jira_client(&state, query.organization_id).await?;
    let statuses = client.get_statuses().await.map_err(|e| {
        tracing::warn!(?e, "failed to fetch Jira statuses");
        ErrorResponse::new(StatusCode::BAD_GATEWAY, "failed to fetch Jira statuses")
    })?;

    let mut views: Vec<jira_client::JiraStatusView> = statuses
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

    // Deduplicate by name (Jira can have duplicate status names across projects)
    views.sort_by(|a, b| a.name.cmp(&b.name));
    views.dedup_by(|a, b| a.name == b.name);

    Ok(Json(views))
}

async fn get_status_mappings(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    axum::extract::Query(query): axum::extract::Query<OrgQuery>,
) -> Result<Json<Vec<JiraStatusMapping>>, ErrorResponse> {
    resolve_org_id(&state, ctx.user.id, query.organization_id).await?;

    let rows = JiraRepository::get_status_mappings(state.pool(), query.organization_id)
        .await
        .map_err(|e| {
            tracing::error!(?e, "failed to load status mappings");
            ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "database error")
        })?;

    let mappings = rows
        .into_iter()
        .map(|r| JiraStatusMapping {
            vk_status_name: r.vk_status_name,
            jira_category_key: r.jira_category_key,
        })
        .collect();

    Ok(Json(mappings))
}

#[instrument(name = "jira.upsert_status_mapping", skip(state, ctx, payload))]
async fn upsert_status_mapping(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    axum::extract::Query(query): axum::extract::Query<OrgQuery>,
    Json(payload): Json<JiraStatusMappingRequest>,
) -> Result<StatusCode, ErrorResponse> {
    resolve_org_id(&state, ctx.user.id, query.organization_id).await?;

    JiraRepository::upsert_status_mapping(
        state.pool(),
        query.organization_id,
        &payload.vk_status_name,
        &payload.jira_category_key,
    )
    .await
    .map_err(|e| {
        tracing::error!(?e, "failed to upsert status mapping");
        ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "database error")
    })?;

    Ok(StatusCode::NO_CONTENT)
}

#[instrument(name = "jira.delete_status_mapping", skip(state, ctx, payload))]
async fn delete_status_mapping(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    axum::extract::Query(query): axum::extract::Query<OrgQuery>,
    Json(payload): Json<JiraStatusMappingDeleteRequest>,
) -> Result<StatusCode, ErrorResponse> {
    resolve_org_id(&state, ctx.user.id, query.organization_id).await?;

    JiraRepository::delete_status_mapping(
        state.pool(),
        query.organization_id,
        &payload.vk_status_name,
    )
    .await
    .map_err(|e| {
        tracing::error!(?e, "failed to delete status mapping");
        ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, "database error")
    })?;

    Ok(StatusCode::NO_CONTENT)
}
