use api_types::{
    CreateIssueRequest, Issue, ListIssuesQuery, ListIssuesResponse, MutationResponse,
    SearchIssuesRequest, UpdateIssueRequest,
};
use axum::{
    Router,
    extract::{Json, Path, Query, State},
    response::Json as ResponseJson,
    routing::{get, post},
};
use db::models::local_issue::LocalIssue;
use deployment::Deployment;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

pub(super) fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/issues", get(list_issues).post(create_issue))
        .route("/issues/search", post(search_issues))
        .route(
            "/issues/{issue_id}",
            get(get_issue).patch(update_issue).delete(delete_issue),
        )
}

async fn list_issues(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ListIssuesQuery>,
) -> Result<ResponseJson<ApiResponse<ListIssuesResponse>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.list_issues(query.project_id).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let issues = LocalIssue::find_by_project(pool, query.project_id).await?;
            let total_count = issues.len();
            let response = ListIssuesResponse {
                issues,
                total_count,
                limit: total_count,
                offset: 0,
            };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn search_issues(
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<SearchIssuesRequest>,
) -> Result<ResponseJson<ApiResponse<ListIssuesResponse>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.search_issues(&request).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let response = LocalIssue::search(pool, &request).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn get_issue(
    State(deployment): State<DeploymentImpl>,
    Path(issue_id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<Issue>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.get_issue(issue_id).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let issue = LocalIssue::find_by_id(pool, issue_id)
                .await?
                .ok_or_else(|| ApiError::BadRequest("Issue not found".into()))?;
            Ok(ResponseJson(ApiResponse::success(issue)))
        }
    }
}

async fn create_issue(
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<CreateIssueRequest>,
) -> Result<ResponseJson<ApiResponse<MutationResponse<Issue>>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.create_issue(&request).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let issue = LocalIssue::create(pool, &request).await?;
            let response = MutationResponse {
                data: issue,
                txid: 0,
            };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn update_issue(
    State(deployment): State<DeploymentImpl>,
    Path(issue_id): Path<Uuid>,
    Json(request): Json<UpdateIssueRequest>,
) -> Result<ResponseJson<ApiResponse<MutationResponse<Issue>>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.update_issue(issue_id, &request).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let issue = LocalIssue::update(pool, issue_id, &request).await?;
            let response = MutationResponse {
                data: issue,
                txid: 0,
            };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn delete_issue(
    State(deployment): State<DeploymentImpl>,
    Path(issue_id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            client.delete_issue(issue_id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            LocalIssue::delete(pool, issue_id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
    }
}
