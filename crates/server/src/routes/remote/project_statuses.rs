use api_types::{
    CreateProjectStatusRequest, ListProjectStatusesResponse, MutationResponse, ProjectStatus,
    UpdateProjectStatusRequest,
};
use axum::{
    Router,
    extract::{Json, Path, Query, State},
    response::Json as ResponseJson,
    routing::get,
};
use db::models::project_status::LocalProjectStatus;
use deployment::Deployment;
use serde::Deserialize;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Deserialize)]
pub(super) struct ListProjectStatusesQuery {
    pub project_id: Uuid,
}

pub(super) fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route(
            "/project-statuses",
            get(list_project_statuses).post(create_project_status),
        )
        .route(
            "/project-statuses/{id}",
            axum::routing::patch(update_project_status).delete(delete_project_status),
        )
}

async fn list_project_statuses(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ListProjectStatusesQuery>,
) -> Result<ResponseJson<ApiResponse<ListProjectStatusesResponse>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.list_project_statuses(query.project_id).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let project_statuses =
                LocalProjectStatus::find_by_project(pool, query.project_id).await?;
            let response = ListProjectStatusesResponse { project_statuses };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn create_project_status(
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<CreateProjectStatusRequest>,
) -> Result<ResponseJson<ApiResponse<MutationResponse<ProjectStatus>>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.create_project_status(&request).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let status = LocalProjectStatus::create(
                pool,
                request.project_id,
                &request.name,
                &request.color,
                request.sort_order,
            )
            .await?;
            let response = MutationResponse {
                data: status,
                txid: 0,
            };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn update_project_status(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateProjectStatusRequest>,
) -> Result<ResponseJson<ApiResponse<MutationResponse<ProjectStatus>>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.update_project_status(id, &request).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let status = LocalProjectStatus::update(
                pool,
                id,
                request.name.as_deref(),
                request.color.as_deref(),
                request.sort_order,
                request.hidden,
            )
            .await?;
            let response = MutationResponse {
                data: status,
                txid: 0,
            };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn delete_project_status(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            client.delete_project_status(id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            LocalProjectStatus::delete(pool, id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
    }
}
