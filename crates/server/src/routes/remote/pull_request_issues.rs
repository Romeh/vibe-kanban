use api_types::{
    CreatePullRequestIssueRequest, ListPullRequestIssuesResponse, MutationResponse,
    PullRequestIssue,
};
use axum::{
    Router,
    extract::{Json, Path, Query, State},
    response::Json as ResponseJson,
    routing::get,
};
use deployment::Deployment;
use serde::Deserialize;
use sqlx::Row;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Deserialize)]
pub(super) struct ListPullRequestIssuesQuery {
    pub pull_request_id: Uuid,
}

pub(super) fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route(
            "/pull-request-issues",
            get(list_pull_request_issues).post(create_pull_request_issue),
        )
        .route(
            "/pull-request-issues/{id}",
            axum::routing::delete(delete_pull_request_issue),
        )
}

fn pri_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<PullRequestIssue, sqlx::Error> {
    Ok(PullRequestIssue {
        id: row.try_get("id")?,
        pull_request_id: row.try_get("pull_request_id")?,
        issue_id: row.try_get("issue_id")?,
    })
}

async fn list_pull_request_issues(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ListPullRequestIssuesQuery>,
) -> Result<ResponseJson<ApiResponse<ListPullRequestIssuesResponse>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client
                .list_pull_request_issues(query.pull_request_id)
                .await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let rows = sqlx::query(
                "SELECT id, pull_request_id, issue_id FROM pull_request_issues WHERE pull_request_id = $1",
            )
            .bind(query.pull_request_id)
            .fetch_all(pool)
            .await?;
            let pull_request_issues: Vec<PullRequestIssue> =
                rows.iter().map(pri_from_row).collect::<Result<_, _>>()?;
            Ok(ResponseJson(ApiResponse::success(
                ListPullRequestIssuesResponse {
                    pull_request_issues,
                },
            )))
        }
    }
}

async fn create_pull_request_issue(
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<CreatePullRequestIssueRequest>,
) -> Result<ResponseJson<ApiResponse<MutationResponse<PullRequestIssue>>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.create_pull_request_issue(&request).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let id = request.id.unwrap_or_else(Uuid::new_v4);
            // In local mode, create a minimal PR record and link it to the issue.
            let pr_id = Uuid::new_v4();
            sqlx::query(
                "INSERT OR IGNORE INTO pull_request_issues (id, pull_request_id, issue_id) VALUES ($1, $2, $3)",
            )
            .bind(id)
            .bind(pr_id)
            .bind(request.issue_id)
            .execute(pool)
            .await?;

            let entity = PullRequestIssue {
                id,
                pull_request_id: pr_id,
                issue_id: request.issue_id,
            };
            Ok(ResponseJson(ApiResponse::success(MutationResponse {
                data: entity,
                txid: 0,
            })))
        }
    }
}

async fn delete_pull_request_issue(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            client.delete_pull_request_issue(id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            sqlx::query("DELETE FROM pull_request_issues WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
    }
}
