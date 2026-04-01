use api_types::{
    CreateProjectRequest, ListProjectsResponse, MutationResponse, Project, UpdateProjectRequest,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json as ResponseJson,
    routing::get,
};
use db::models::{project::Project as LocalProject, project_status::LocalProjectStatus};
use deployment::Deployment;
use serde::Deserialize;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

/// Fixed UUID for the local organization.
const LOCAL_ORG_ID: &str = "00000000-0000-0000-0000-000000000001";

#[derive(Debug, Deserialize)]
pub(super) struct ListRemoteProjectsQuery {
    pub organization_id: Uuid,
}

pub(super) fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/projects", get(list_remote_projects).post(create_project))
        .route(
            "/projects/{project_id}",
            get(get_remote_project)
                .patch(update_project)
                .delete(delete_project),
        )
}

fn local_project_to_api(p: &LocalProject) -> Project {
    let org_id: Uuid = LOCAL_ORG_ID.parse().unwrap();
    Project {
        id: p.id,
        organization_id: p.organization_id.unwrap_or(org_id),
        name: p.name.clone(),
        color: p.color.as_deref().unwrap_or("#3b82f6").to_string(),
        sort_order: p.sort_order.unwrap_or(0),
        created_at: p.created_at,
        updated_at: p.updated_at,
    }
}

async fn list_remote_projects(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ListRemoteProjectsQuery>,
) -> Result<ResponseJson<ApiResponse<ListProjectsResponse>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.list_remote_projects(query.organization_id).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let local_projects = LocalProject::find_all(pool).await?;
            let projects: Vec<Project> = local_projects.iter().map(local_project_to_api).collect();
            let response = ListProjectsResponse { projects };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn get_remote_project(
    State(deployment): State<DeploymentImpl>,
    Path(project_id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<Project>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let project = client.get_remote_project(project_id).await?;
            Ok(ResponseJson(ApiResponse::success(project)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let project = LocalProject::find_by_id(pool, project_id)
                .await?
                .map(|p| local_project_to_api(&p))
                .ok_or_else(|| ApiError::BadRequest("Project not found".into()))?;
            Ok(ResponseJson(ApiResponse::success(project)))
        }
    }
}

async fn create_project(
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<CreateProjectRequest>,
) -> Result<ResponseJson<MutationResponse<Project>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.create_project(&request).await?;
            Ok(ResponseJson(response))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let id = request.id.unwrap_or_else(Uuid::new_v4);
            let org_id: Uuid = LOCAL_ORG_ID.parse().unwrap();

            // Create the project in local SQLite
            sqlx::query(
                "INSERT INTO projects (id, name, organization_id, identifier, color, sort_order) VALUES ($1, $2, $3, $4, $5, 0)",
            )
            .bind(id)
            .bind(&request.name)
            .bind(org_id)
            .bind(request.name.chars().take(3).collect::<String>().to_uppercase())
            .bind(&request.color)
            .execute(pool)
            .await?;

            // Auto-create default project statuses
            LocalProjectStatus::create_defaults(pool, id).await?;

            let project = Project {
                id,
                organization_id: org_id,
                name: request.name,
                color: request.color,
                sort_order: 0,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };

            Ok(ResponseJson(MutationResponse {
                data: project,
                txid: 0,
            }))
        }
    }
}

async fn update_project(
    State(deployment): State<DeploymentImpl>,
    Path(project_id): Path<Uuid>,
    Json(request): Json<UpdateProjectRequest>,
) -> Result<ResponseJson<MutationResponse<Project>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.update_project(project_id, &request).await?;
            Ok(ResponseJson(response))
        }
        Err(_) => {
            let pool = &deployment.db().pool;

            sqlx::query(
                "UPDATE projects SET name = COALESCE($2, name), color = COALESCE($3, color), sort_order = COALESCE($4, sort_order), updated_at = datetime('now', 'subsec') WHERE id = $1",
            )
            .bind(project_id)
            .bind(&request.name)
            .bind(&request.color)
            .bind(request.sort_order)
            .execute(pool)
            .await?;

            let project = LocalProject::find_by_id(pool, project_id)
                .await?
                .map(|p| local_project_to_api(&p))
                .ok_or_else(|| ApiError::BadRequest("Project not found".into()))?;

            Ok(ResponseJson(MutationResponse {
                data: project,
                txid: 0,
            }))
        }
    }
}

async fn delete_project(
    State(deployment): State<DeploymentImpl>,
    Path(project_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            client.delete_project(project_id).await?;
            Ok(StatusCode::NO_CONTENT)
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            sqlx::query("DELETE FROM projects WHERE id = $1")
                .bind(project_id)
                .execute(pool)
                .await?;
            Ok(StatusCode::NO_CONTENT)
        }
    }
}
