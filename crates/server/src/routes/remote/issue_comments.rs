use api_types::{
    CreateIssueCommentRequest, IssueComment, ListIssueCommentsResponse, MutationResponse,
    UpdateIssueCommentRequest,
};
use axum::{
    Router,
    extract::{Json, Path, Query, State},
    response::Json as ResponseJson,
    routing::get,
};
use db::models::issue_comment::IssueComment as LocalIssueComment;
use deployment::Deployment;
use serde::Deserialize;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Deserialize)]
pub(super) struct ListIssueCommentsQuery {
    pub issue_id: Uuid,
}

pub(super) fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route(
            "/issue-comments",
            get(list_issue_comments).post(create_issue_comment),
        )
        .route(
            "/issue-comments/{id}",
            axum::routing::patch(update_issue_comment).delete(delete_issue_comment),
        )
}

fn local_to_api(c: &LocalIssueComment) -> IssueComment {
    IssueComment {
        id: c.id,
        issue_id: c.issue_id,
        author_id: c.author_id,
        parent_id: c.parent_id,
        message: c.message.clone(),
        created_at: c.created_at,
        updated_at: c.updated_at,
    }
}

async fn list_issue_comments(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ListIssueCommentsQuery>,
) -> Result<ResponseJson<ApiResponse<ListIssueCommentsResponse>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.list_issue_comments(query.issue_id).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let local_comments = LocalIssueComment::find_by_issue(pool, query.issue_id).await?;
            let issue_comments: Vec<IssueComment> =
                local_comments.iter().map(local_to_api).collect();
            let response = ListIssueCommentsResponse { issue_comments };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn create_issue_comment(
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<CreateIssueCommentRequest>,
) -> Result<ResponseJson<ApiResponse<MutationResponse<IssueComment>>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.create_issue_comment(&request).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let local_comment = LocalIssueComment::create(
                pool,
                request.issue_id,
                None, // author_id — not available in local mode
                request.parent_id,
                &request.message,
            )
            .await?;
            let response = MutationResponse {
                data: local_to_api(&local_comment),
                txid: 0,
            };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn update_issue_comment(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateIssueCommentRequest>,
) -> Result<ResponseJson<ApiResponse<MutationResponse<IssueComment>>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.update_issue_comment(id, &request).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let message = request
                .message
                .as_deref()
                .ok_or_else(|| ApiError::BadRequest("message is required".into()))?;
            let local_comment = LocalIssueComment::update(pool, id, message).await?;
            let response = MutationResponse {
                data: local_to_api(&local_comment),
                txid: 0,
            };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn delete_issue_comment(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            client.delete_issue_comment(id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            LocalIssueComment::delete(pool, id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
    }
}
