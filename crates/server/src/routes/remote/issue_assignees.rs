use api_types::{
    CreateIssueAssigneeRequest, IssueAssignee, ListIssueAssigneesResponse, MutationResponse,
};
use axum::{
    Router,
    extract::{Json, Path, Query, State},
    response::Json as ResponseJson,
    routing::get,
};
use db::models::issue_assignee::IssueAssignee as LocalIssueAssignee;
use deployment::Deployment;
use serde::Deserialize;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Deserialize)]
pub(super) struct ListIssueAssigneesQuery {
    pub issue_id: Uuid,
}

pub(super) fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route(
            "/issue-assignees",
            get(list_issue_assignees).post(create_issue_assignee),
        )
        .route(
            "/issue-assignees/{issue_assignee_id}",
            get(get_issue_assignee).delete(delete_issue_assignee),
        )
}

fn local_to_api(a: &LocalIssueAssignee) -> IssueAssignee {
    IssueAssignee {
        id: a.id,
        issue_id: a.issue_id,
        user_id: a.user_id,
        assigned_at: a.assigned_at,
    }
}

async fn list_issue_assignees(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ListIssueAssigneesQuery>,
) -> Result<ResponseJson<ApiResponse<ListIssueAssigneesResponse>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.list_issue_assignees(query.issue_id).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let local_assignees = LocalIssueAssignee::find_by_issue(pool, query.issue_id).await?;
            let issue_assignees: Vec<IssueAssignee> =
                local_assignees.iter().map(local_to_api).collect();
            let response = ListIssueAssigneesResponse { issue_assignees };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn get_issue_assignee(
    State(deployment): State<DeploymentImpl>,
    Path(issue_assignee_id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<IssueAssignee>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.get_issue_assignee(issue_assignee_id).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let row = sqlx::query(
                "SELECT id, issue_id, user_id, assigned_at FROM issue_assignees WHERE id = $1",
            )
            .bind(issue_assignee_id)
            .fetch_optional(pool)
            .await?;
            match row {
                Some(row) => {
                    use sqlx::Row;
                    let assignee = IssueAssignee {
                        id: row.try_get("id")?,
                        issue_id: row.try_get("issue_id")?,
                        user_id: row.try_get("user_id")?,
                        assigned_at: row.try_get("assigned_at")?,
                    };
                    Ok(ResponseJson(ApiResponse::success(assignee)))
                }
                None => Err(ApiError::BadRequest("Issue assignee not found".into())),
            }
        }
    }
}

async fn create_issue_assignee(
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<CreateIssueAssigneeRequest>,
) -> Result<ResponseJson<ApiResponse<MutationResponse<IssueAssignee>>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.create_issue_assignee(&request).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let local_assignee =
                LocalIssueAssignee::create(pool, request.issue_id, request.user_id).await?;
            let response = MutationResponse {
                data: local_to_api(&local_assignee),
                txid: 0,
            };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn delete_issue_assignee(
    State(deployment): State<DeploymentImpl>,
    Path(issue_assignee_id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            client.delete_issue_assignee(issue_assignee_id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            LocalIssueAssignee::delete(pool, issue_assignee_id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
    }
}
