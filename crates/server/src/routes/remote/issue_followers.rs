use api_types::{
    CreateIssueFollowerRequest, IssueFollower, ListIssueFollowersResponse, MutationResponse,
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
pub(super) struct ListIssueFollowersQuery {
    pub issue_id: Uuid,
}

pub(super) fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route(
            "/issue-followers",
            get(list_issue_followers).post(create_issue_follower),
        )
        .route(
            "/issue-followers/{id}",
            axum::routing::delete(delete_issue_follower),
        )
}

fn follower_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<IssueFollower, sqlx::Error> {
    Ok(IssueFollower {
        id: row.try_get("id")?,
        issue_id: row.try_get("issue_id")?,
        user_id: row.try_get("user_id")?,
    })
}

async fn list_issue_followers(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ListIssueFollowersQuery>,
) -> Result<ResponseJson<ApiResponse<ListIssueFollowersResponse>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.list_issue_followers(query.issue_id).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let rows = sqlx::query(
                "SELECT id, issue_id, user_id FROM issue_followers WHERE issue_id = $1",
            )
            .bind(query.issue_id)
            .fetch_all(pool)
            .await?;
            let issue_followers: Vec<IssueFollower> = rows
                .iter()
                .map(follower_from_row)
                .collect::<Result<_, _>>()?;
            Ok(ResponseJson(ApiResponse::success(
                ListIssueFollowersResponse { issue_followers },
            )))
        }
    }
}

async fn create_issue_follower(
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<CreateIssueFollowerRequest>,
) -> Result<ResponseJson<ApiResponse<MutationResponse<IssueFollower>>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.create_issue_follower(&request).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let id = request.id.unwrap_or_else(Uuid::new_v4);
            sqlx::query(
                "INSERT OR IGNORE INTO issue_followers (id, issue_id, user_id) VALUES ($1, $2, $3)",
            )
            .bind(id)
            .bind(request.issue_id)
            .bind(request.user_id)
            .execute(pool)
            .await?;

            let follower = IssueFollower {
                id,
                issue_id: request.issue_id,
                user_id: request.user_id,
            };
            Ok(ResponseJson(ApiResponse::success(MutationResponse {
                data: follower,
                txid: 0,
            })))
        }
    }
}

async fn delete_issue_follower(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            client.delete_issue_follower(id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            sqlx::query("DELETE FROM issue_followers WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
    }
}
