use api_types::{
    CreateIssueCommentReactionRequest, IssueCommentReaction, ListIssueCommentReactionsResponse,
    MutationResponse,
};
use axum::{
    Router,
    extract::{Json, Path, Query, State},
    response::Json as ResponseJson,
    routing::get,
};
use chrono::Utc;
use deployment::Deployment;
use serde::Deserialize;
use sqlx::Row;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Deserialize)]
pub(super) struct ListIssueCommentReactionsQuery {
    pub comment_id: Uuid,
}

pub(super) fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route(
            "/issue-comment-reactions",
            get(list_issue_comment_reactions).post(create_issue_comment_reaction),
        )
        .route(
            "/issue-comment-reactions/{id}",
            axum::routing::delete(delete_issue_comment_reaction),
        )
}

fn reaction_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<IssueCommentReaction, sqlx::Error> {
    Ok(IssueCommentReaction {
        id: row.try_get("id")?,
        comment_id: row.try_get("comment_id")?,
        user_id: row.try_get("user_id")?,
        emoji: row.try_get("emoji")?,
        created_at: row.try_get("created_at")?,
    })
}

async fn list_issue_comment_reactions(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ListIssueCommentReactionsQuery>,
) -> Result<ResponseJson<ApiResponse<ListIssueCommentReactionsResponse>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client
                .list_issue_comment_reactions(query.comment_id)
                .await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let rows = sqlx::query(
                "SELECT id, comment_id, user_id, emoji, created_at FROM issue_comment_reactions WHERE comment_id = $1",
            )
            .bind(query.comment_id)
            .fetch_all(pool)
            .await?;
            let issue_comment_reactions: Vec<IssueCommentReaction> = rows
                .iter()
                .map(reaction_from_row)
                .collect::<Result<_, _>>()?;
            Ok(ResponseJson(ApiResponse::success(
                ListIssueCommentReactionsResponse {
                    issue_comment_reactions,
                },
            )))
        }
    }
}

async fn create_issue_comment_reaction(
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<CreateIssueCommentReactionRequest>,
) -> Result<ResponseJson<ApiResponse<MutationResponse<IssueCommentReaction>>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.create_issue_comment_reaction(&request).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let id = request.id.unwrap_or_else(Uuid::new_v4);
            let now = Utc::now();
            sqlx::query(
                "INSERT OR IGNORE INTO issue_comment_reactions (id, comment_id, user_id, emoji) VALUES ($1, $2, $3, $4)",
            )
            .bind(id)
            .bind(request.comment_id)
            .bind(Uuid::nil()) // local single-user
            .bind(&request.emoji)
            .execute(pool)
            .await?;

            let reaction = IssueCommentReaction {
                id,
                comment_id: request.comment_id,
                user_id: Uuid::nil(),
                emoji: request.emoji,
                created_at: now,
            };
            Ok(ResponseJson(ApiResponse::success(MutationResponse {
                data: reaction,
                txid: 0,
            })))
        }
    }
}

async fn delete_issue_comment_reaction(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            client.delete_issue_comment_reaction(id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            sqlx::query("DELETE FROM issue_comment_reactions WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
    }
}
