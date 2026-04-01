use api_types::{CreateTagRequest, ListTagsResponse, MutationResponse, Tag, UpdateTagRequest};
use axum::{
    Router,
    extract::{Json, Path, Query, State},
    response::Json as ResponseJson,
    routing::get,
};
use db::models::project_tag::ProjectTag;
use deployment::Deployment;
use serde::Deserialize;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Deserialize)]
pub(super) struct ListTagsQuery {
    pub project_id: Uuid,
}

pub(super) fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/tags", get(list_tags).post(create_tag))
        .route(
            "/tags/{tag_id}",
            get(get_tag).patch(update_tag).delete(delete_tag),
        )
}

fn project_tag_to_api(t: &ProjectTag) -> Tag {
    Tag {
        id: t.id,
        project_id: t.project_id,
        name: t.name.clone(),
        color: t.color.clone(),
    }
}

async fn list_tags(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ListTagsQuery>,
) -> Result<ResponseJson<ApiResponse<ListTagsResponse>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.list_tags(query.project_id).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let local_tags = ProjectTag::find_by_project(pool, query.project_id).await?;
            let tags: Vec<Tag> = local_tags.iter().map(project_tag_to_api).collect();
            let response = ListTagsResponse { tags };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn get_tag(
    State(deployment): State<DeploymentImpl>,
    Path(tag_id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<Tag>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.get_tag(tag_id).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            // No find_by_id on ProjectTag, so query directly
            let pool = &deployment.db().pool;
            let row =
                sqlx::query("SELECT id, project_id, name, color FROM project_tags WHERE id = $1")
                    .bind(tag_id)
                    .fetch_optional(pool)
                    .await?;
            match row {
                Some(row) => {
                    use sqlx::Row;
                    let tag = Tag {
                        id: row.try_get("id")?,
                        project_id: row.try_get("project_id")?,
                        name: row.try_get("name")?,
                        color: row.try_get("color")?,
                    };
                    Ok(ResponseJson(ApiResponse::success(tag)))
                }
                None => Err(ApiError::BadRequest("Tag not found".into())),
            }
        }
    }
}

async fn create_tag(
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<CreateTagRequest>,
) -> Result<ResponseJson<ApiResponse<MutationResponse<Tag>>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.create_tag(&request).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let local_tag =
                ProjectTag::create(pool, request.project_id, &request.name, &request.color).await?;
            let response = MutationResponse {
                data: project_tag_to_api(&local_tag),
                txid: 0,
            };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn update_tag(
    State(deployment): State<DeploymentImpl>,
    Path(tag_id): Path<Uuid>,
    Json(request): Json<UpdateTagRequest>,
) -> Result<ResponseJson<ApiResponse<MutationResponse<Tag>>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.update_tag(tag_id, &request).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            // ProjectTag::update requires both name and color; use existing values as defaults
            let row =
                sqlx::query("SELECT id, project_id, name, color FROM project_tags WHERE id = $1")
                    .bind(tag_id)
                    .fetch_optional(pool)
                    .await?;
            let existing = match row {
                Some(row) => {
                    use sqlx::Row;
                    (
                        row.try_get::<String, _>("name")?,
                        row.try_get::<String, _>("color")?,
                    )
                }
                None => return Err(ApiError::BadRequest("Tag not found".into())),
            };
            let name = request.name.as_deref().unwrap_or(&existing.0);
            let color = request.color.as_deref().unwrap_or(&existing.1);
            let local_tag = ProjectTag::update(pool, tag_id, name, color).await?;
            let response = MutationResponse {
                data: project_tag_to_api(&local_tag),
                txid: 0,
            };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn delete_tag(
    State(deployment): State<DeploymentImpl>,
    Path(tag_id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            client.delete_tag(tag_id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            ProjectTag::delete(pool, tag_id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
    }
}
