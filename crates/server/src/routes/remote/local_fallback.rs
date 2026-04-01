//! Local fallback endpoints that return data in the format expected by the
//! Electric fallback sync layer: `{ "<table_name>": [...rows...] }`.
//!
//! These are only used when VK_SHARED_API_BASE is not set (local standalone mode).
//! In remote mode, Electric sync handles data loading.

use axum::{
    Router,
    extract::{Query, State},
    response::Json as ResponseJson,
    routing::get,
};
use chrono::Utc;
use db::models::{
    issue_assignee::IssueAssignee as IssueAssigneeModel,
    issue_comment::IssueComment as IssueCommentModel,
    issue_relationship::IssueRelationship as IssueRelationshipModel,
    issue_tag::IssueTag as IssueTagModel, local_issue::LocalIssue, project::Project,
    project_status::LocalProjectStatus, project_tag::ProjectTag,
};
use deployment::Deployment;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

const LOCAL_ORG_ID: &str = "00000000-0000-0000-0000-000000000001";
const LOCAL_USER_ID: &str = "00000000-0000-0000-0000-000000000002";

#[derive(Debug, Deserialize)]
struct OrgQuery {
    #[allow(dead_code)]
    organization_id: Option<Uuid>,
    #[allow(dead_code)]
    user_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
struct ProjectQuery {
    project_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
struct IssueQuery {
    issue_id: Option<Uuid>,
}

pub(super) fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/fallback/projects", get(fallback_projects))
        .route("/fallback/project-statuses", get(fallback_project_statuses))
        .route("/fallback/issues", get(fallback_issues))
        .route("/fallback/tags", get(fallback_tags))
        .route("/fallback/issue-assignees", get(fallback_issue_assignees))
        .route("/fallback/issue-tags", get(fallback_issue_tags))
        .route(
            "/fallback/issue-relationships",
            get(fallback_issue_relationships),
        )
        .route("/fallback/issue-comments", get(fallback_issue_comments))
        .route(
            "/fallback/issue-comment-reactions",
            get(fallback_issue_comment_reactions),
        )
        .route("/fallback/issue-followers", get(fallback_issue_followers))
        .route(
            "/fallback/pull-requests",
            get(fallback_empty_array("pull_requests")),
        )
        .route(
            "/fallback/pull-request-issues",
            get(fallback_empty_array("pull_request_issues")),
        )
        .route(
            "/fallback/notifications",
            get(fallback_empty_array("notifications")),
        )
        .route(
            "/fallback/organization-members",
            get(fallback_organization_members),
        )
        .route("/fallback/users", get(fallback_users))
        .route("/fallback/user-workspaces", get(fallback_user_workspaces))
        .route(
            "/fallback/project-workspaces",
            get(fallback_project_workspaces),
        )
}

fn fallback_empty_array(
    table: &'static str,
) -> impl Fn() -> std::pin::Pin<
    Box<dyn std::future::Future<Output = ResponseJson<serde_json::Value>> + Send>,
> + Clone {
    move || {
        let t = table;
        Box::pin(async move { ResponseJson(json!({ t: [] })) })
    }
}

async fn fallback_projects(
    State(deployment): State<DeploymentImpl>,
    Query(_q): Query<OrgQuery>,
) -> Result<ResponseJson<serde_json::Value>, ApiError> {
    let pool = &deployment.db().pool;
    let local_projects = Project::find_all(pool).await?;
    let org_id: Uuid = LOCAL_ORG_ID.parse().unwrap();

    let projects: Vec<serde_json::Value> = local_projects
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "organization_id": p.organization_id.unwrap_or(org_id),
                "name": p.name,
                "color": p.color.as_deref().unwrap_or("#3b82f6"),
                "sort_order": p.sort_order.unwrap_or(0),
                "created_at": p.created_at,
                "updated_at": p.updated_at,
            })
        })
        .collect();

    Ok(ResponseJson(json!({ "projects": projects })))
}

async fn fallback_project_statuses(
    State(deployment): State<DeploymentImpl>,
    Query(q): Query<ProjectQuery>,
) -> Result<ResponseJson<serde_json::Value>, ApiError> {
    let pool = &deployment.db().pool;
    let statuses = match q.project_id {
        Some(pid) => LocalProjectStatus::find_by_project(pool, pid).await?,
        None => vec![],
    };
    Ok(ResponseJson(json!({ "project_statuses": statuses })))
}

async fn fallback_issues(
    State(deployment): State<DeploymentImpl>,
    Query(q): Query<ProjectQuery>,
) -> Result<ResponseJson<serde_json::Value>, ApiError> {
    let pool = &deployment.db().pool;
    let issues = match q.project_id {
        Some(pid) => LocalIssue::find_by_project(pool, pid).await?,
        None => vec![],
    };
    Ok(ResponseJson(json!({ "issues": issues })))
}

async fn fallback_tags(
    State(deployment): State<DeploymentImpl>,
    Query(q): Query<ProjectQuery>,
) -> Result<ResponseJson<serde_json::Value>, ApiError> {
    let pool = &deployment.db().pool;
    let tags = match q.project_id {
        Some(pid) => ProjectTag::find_by_project(pool, pid).await?,
        None => vec![],
    };
    Ok(ResponseJson(json!({ "tags": tags })))
}

async fn fallback_issue_assignees(
    State(deployment): State<DeploymentImpl>,
    Query(q): Query<ProjectQuery>,
) -> Result<ResponseJson<serde_json::Value>, ApiError> {
    let pool = &deployment.db().pool;
    let assignees = match q.project_id {
        Some(pid) => IssueAssigneeModel::find_by_project_issues(pool, pid).await?,
        None => vec![],
    };
    Ok(ResponseJson(json!({ "issue_assignees": assignees })))
}

async fn fallback_issue_tags(
    State(deployment): State<DeploymentImpl>,
    Query(q): Query<ProjectQuery>,
) -> Result<ResponseJson<serde_json::Value>, ApiError> {
    let pool = &deployment.db().pool;
    let tags = match q.project_id {
        Some(pid) => IssueTagModel::find_by_project_issues(pool, pid).await?,
        None => vec![],
    };
    Ok(ResponseJson(json!({ "issue_tags": tags })))
}

async fn fallback_issue_relationships(
    State(deployment): State<DeploymentImpl>,
    Query(q): Query<ProjectQuery>,
) -> Result<ResponseJson<serde_json::Value>, ApiError> {
    let pool = &deployment.db().pool;
    let relationships = match q.project_id {
        Some(pid) => IssueRelationshipModel::find_by_project_issues(pool, pid).await?,
        None => vec![],
    };
    Ok(ResponseJson(
        json!({ "issue_relationships": relationships }),
    ))
}

async fn fallback_issue_comments(
    State(deployment): State<DeploymentImpl>,
    Query(q): Query<IssueQuery>,
) -> Result<ResponseJson<serde_json::Value>, ApiError> {
    let pool = &deployment.db().pool;
    let comments = match q.issue_id {
        Some(iid) => IssueCommentModel::find_by_issue(pool, iid).await?,
        None => vec![],
    };
    Ok(ResponseJson(json!({ "issue_comments": comments })))
}

async fn fallback_organization_members(
    Query(_q): Query<OrgQuery>,
) -> ResponseJson<serde_json::Value> {
    let org_id: Uuid = LOCAL_ORG_ID.parse().unwrap();
    let user_id: Uuid = LOCAL_USER_ID.parse().unwrap();
    let now = Utc::now();
    ResponseJson(json!({
        "organization_member_metadata": [{
            "organization_id": org_id,
            "user_id": user_id,
            "role": "ADMIN",
            "joined_at": now,
            "last_seen_at": now,
        }]
    }))
}

async fn fallback_users(Query(_q): Query<OrgQuery>) -> ResponseJson<serde_json::Value> {
    let user_id: Uuid = LOCAL_USER_ID.parse().unwrap();
    let now = Utc::now();
    ResponseJson(json!({
        "users": [{
            "id": user_id,
            "email": "local@localhost",
            "first_name": "Local",
            "last_name": "User",
            "username": "local-user",
            "created_at": now,
            "updated_at": now,
        }]
    }))
}

async fn fallback_project_workspaces(
    State(deployment): State<DeploymentImpl>,
    Query(q): Query<ProjectQuery>,
) -> Result<ResponseJson<serde_json::Value>, ApiError> {
    let pool = &deployment.db().pool;
    let user_id: Uuid = LOCAL_USER_ID.parse().unwrap();

    let workspaces: Vec<serde_json::Value> = match q.project_id {
        Some(pid) => {
            let rows = sqlx::query(
                "SELECT id, issue_id, project_id, name, archived, created_at, updated_at \
                 FROM workspaces WHERE project_id = $1",
            )
            .bind(pid)
            .fetch_all(pool)
            .await?;

            rows.iter()
                .map(|row| {
                    use sqlx::Row;
                    json!({
                        "id": row.get::<Uuid, _>("id"),
                        "project_id": row.get::<Uuid, _>("project_id"),
                        "owner_user_id": user_id,
                        "issue_id": row.get::<Option<Uuid>, _>("issue_id"),
                        "local_workspace_id": row.get::<Uuid, _>("id"),
                        "name": row.get::<Option<String>, _>("name"),
                        "archived": row.get::<bool, _>("archived"),
                        "files_changed": Option::<i32>::None,
                        "lines_added": Option::<i32>::None,
                        "lines_removed": Option::<i32>::None,
                        "created_at": row.get::<String, _>("created_at"),
                        "updated_at": row.get::<String, _>("updated_at"),
                    })
                })
                .collect()
        }
        None => vec![],
    };

    Ok(ResponseJson(json!({ "workspaces": workspaces })))
}

async fn fallback_issue_followers(
    State(deployment): State<DeploymentImpl>,
    Query(q): Query<ProjectQuery>,
) -> Result<ResponseJson<serde_json::Value>, ApiError> {
    let pool = &deployment.db().pool;
    let followers: Vec<serde_json::Value> = match q.project_id {
        Some(pid) => {
            let rows = sqlx::query(
                "SELECT f.id, f.issue_id, f.user_id \
                 FROM issue_followers f \
                 INNER JOIN issues ON issues.id = f.issue_id \
                 WHERE issues.project_id = $1",
            )
            .bind(pid)
            .fetch_all(pool)
            .await?;

            rows.iter()
                .map(|row| {
                    use sqlx::Row;
                    json!({
                        "id": row.get::<Uuid, _>("id"),
                        "issue_id": row.get::<Uuid, _>("issue_id"),
                        "user_id": row.get::<Uuid, _>("user_id"),
                    })
                })
                .collect()
        }
        None => vec![],
    };

    Ok(ResponseJson(json!({ "issue_followers": followers })))
}

async fn fallback_issue_comment_reactions(
    State(deployment): State<DeploymentImpl>,
    Query(q): Query<IssueQuery>,
) -> Result<ResponseJson<serde_json::Value>, ApiError> {
    let pool = &deployment.db().pool;
    let reactions: Vec<serde_json::Value> = match q.issue_id {
        Some(iid) => {
            let rows = sqlx::query(
                "SELECT r.id, r.comment_id, r.user_id, r.emoji, r.created_at \
                 FROM issue_comment_reactions r \
                 INNER JOIN issue_comments c ON c.id = r.comment_id \
                 WHERE c.issue_id = $1",
            )
            .bind(iid)
            .fetch_all(pool)
            .await?;

            rows.iter()
                .map(|row| {
                    use sqlx::Row;
                    json!({
                        "id": row.get::<Uuid, _>("id"),
                        "comment_id": row.get::<Uuid, _>("comment_id"),
                        "user_id": row.get::<Uuid, _>("user_id"),
                        "emoji": row.get::<String, _>("emoji"),
                        "created_at": row.get::<String, _>("created_at"),
                    })
                })
                .collect()
        }
        None => vec![],
    };

    Ok(ResponseJson(
        json!({ "issue_comment_reactions": reactions }),
    ))
}

async fn fallback_user_workspaces(
    State(deployment): State<DeploymentImpl>,
    Query(_q): Query<OrgQuery>,
) -> Result<ResponseJson<serde_json::Value>, ApiError> {
    let pool = &deployment.db().pool;
    let user_id: Uuid = LOCAL_USER_ID.parse().unwrap();

    let rows = sqlx::query(
        "SELECT id, issue_id, project_id, name, archived, created_at, updated_at \
         FROM workspaces WHERE project_id IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    let workspaces: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            use sqlx::Row;
            json!({
                "id": row.get::<Uuid, _>("id"),
                "project_id": row.get::<Uuid, _>("project_id"),
                "owner_user_id": user_id,
                "issue_id": row.get::<Option<Uuid>, _>("issue_id"),
                "local_workspace_id": row.get::<Uuid, _>("id"),
                "name": row.get::<Option<String>, _>("name"),
                "archived": row.get::<bool, _>("archived"),
                "files_changed": Option::<i32>::None,
                "lines_added": Option::<i32>::None,
                "lines_removed": Option::<i32>::None,
                "created_at": row.get::<String, _>("created_at"),
                "updated_at": row.get::<String, _>("updated_at"),
            })
        })
        .collect();

    Ok(ResponseJson(json!({ "workspaces": workspaces })))
}
