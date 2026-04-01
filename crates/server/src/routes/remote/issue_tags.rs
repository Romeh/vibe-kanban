use api_types::{CreateIssueTagRequest, IssueTag, ListIssueTagsResponse, MutationResponse};
use axum::{
    Router,
    extract::{Json, Path, Query, State},
    response::Json as ResponseJson,
    routing::get,
};
use db::models::issue_tag::IssueTag as LocalIssueTag;
use deployment::Deployment;
use serde::Deserialize;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Deserialize)]
pub(super) struct ListIssueTagsQuery {
    pub issue_id: Uuid,
}

pub(super) fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/issue-tags", get(list_issue_tags).post(create_issue_tag))
        .route(
            "/issue-tags/{issue_tag_id}",
            get(get_issue_tag).delete(delete_issue_tag),
        )
}

fn local_to_api(t: &LocalIssueTag) -> IssueTag {
    IssueTag {
        id: t.id,
        issue_id: t.issue_id,
        tag_id: t.tag_id,
    }
}

async fn list_issue_tags(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ListIssueTagsQuery>,
) -> Result<ResponseJson<ApiResponse<ListIssueTagsResponse>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.list_issue_tags(query.issue_id).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let local_tags = LocalIssueTag::find_by_issue(pool, query.issue_id).await?;
            let issue_tags: Vec<IssueTag> = local_tags.iter().map(local_to_api).collect();
            let response = ListIssueTagsResponse { issue_tags };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn get_issue_tag(
    State(deployment): State<DeploymentImpl>,
    Path(issue_tag_id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<IssueTag>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.get_issue_tag(issue_tag_id).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let row = sqlx::query("SELECT id, issue_id, tag_id FROM issue_tags WHERE id = $1")
                .bind(issue_tag_id)
                .fetch_optional(pool)
                .await?;
            match row {
                Some(row) => {
                    use sqlx::Row;
                    let tag = IssueTag {
                        id: row.try_get("id")?,
                        issue_id: row.try_get("issue_id")?,
                        tag_id: row.try_get("tag_id")?,
                    };
                    Ok(ResponseJson(ApiResponse::success(tag)))
                }
                None => Err(ApiError::BadRequest("Issue tag not found".into())),
            }
        }
    }
}

async fn create_issue_tag(
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<CreateIssueTagRequest>,
) -> Result<ResponseJson<ApiResponse<MutationResponse<IssueTag>>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.create_issue_tag(&request).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let local_tag = LocalIssueTag::create(pool, request.issue_id, request.tag_id).await?;
            let response = MutationResponse {
                data: local_to_api(&local_tag),
                txid: 0,
            };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn delete_issue_tag(
    State(deployment): State<DeploymentImpl>,
    Path(issue_tag_id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            client.delete_issue_tag(issue_tag_id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            LocalIssueTag::delete(pool, issue_tag_id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
    }
}
