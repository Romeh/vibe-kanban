use api_types::{
    CreateIssueRelationshipRequest, IssueRelationship, ListIssueRelationshipsQuery,
    ListIssueRelationshipsResponse, MutationResponse,
};
use axum::{
    Router,
    extract::{Json, Path, Query, State},
    response::Json as ResponseJson,
    routing::get,
};
use db::models::issue_relationship::{
    IssueRelationship as LocalIssueRelationship, IssueRelationshipType as LocalRelType,
};
use deployment::Deployment;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

pub(super) fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route(
            "/issue-relationships",
            get(list_issue_relationships).post(create_issue_relationship),
        )
        .route(
            "/issue-relationships/{relationship_id}",
            axum::routing::delete(delete_issue_relationship),
        )
}

fn local_to_api(r: &LocalIssueRelationship) -> IssueRelationship {
    let relationship_type = match r.relationship_type {
        LocalRelType::Blocking => api_types::IssueRelationshipType::Blocking,
        LocalRelType::Related => api_types::IssueRelationshipType::Related,
        LocalRelType::HasDuplicate => api_types::IssueRelationshipType::HasDuplicate,
    };
    IssueRelationship {
        id: r.id,
        issue_id: r.issue_id,
        related_issue_id: r.related_issue_id,
        relationship_type,
        created_at: r.created_at,
    }
}

fn api_rel_type_to_local(t: api_types::IssueRelationshipType) -> LocalRelType {
    match t {
        api_types::IssueRelationshipType::Blocking => LocalRelType::Blocking,
        api_types::IssueRelationshipType::Related => LocalRelType::Related,
        api_types::IssueRelationshipType::HasDuplicate => LocalRelType::HasDuplicate,
    }
}

async fn list_issue_relationships(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ListIssueRelationshipsQuery>,
) -> Result<ResponseJson<ApiResponse<ListIssueRelationshipsResponse>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.list_issue_relationships(query.issue_id).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let local_rels = LocalIssueRelationship::find_by_issue(pool, query.issue_id).await?;
            let issue_relationships: Vec<IssueRelationship> =
                local_rels.iter().map(local_to_api).collect();
            let response = ListIssueRelationshipsResponse {
                issue_relationships,
            };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn create_issue_relationship(
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<CreateIssueRelationshipRequest>,
) -> Result<ResponseJson<ApiResponse<MutationResponse<IssueRelationship>>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.create_issue_relationship(&request).await?;
            Ok(ResponseJson(ApiResponse::success(response)))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            let local_rel = LocalIssueRelationship::create(
                pool,
                request.issue_id,
                request.related_issue_id,
                api_rel_type_to_local(request.relationship_type),
            )
            .await?;
            let response = MutationResponse {
                data: local_to_api(&local_rel),
                txid: 0,
            };
            Ok(ResponseJson(ApiResponse::success(response)))
        }
    }
}

async fn delete_issue_relationship(
    State(deployment): State<DeploymentImpl>,
    Path(relationship_id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            client.delete_issue_relationship(relationship_id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
        Err(_) => {
            let pool = &deployment.db().pool;
            LocalIssueRelationship::delete(pool, relationship_id).await?;
            Ok(ResponseJson(ApiResponse::success(())))
        }
    }
}
