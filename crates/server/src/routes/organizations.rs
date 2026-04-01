use api_types::{
    AcceptInvitationResponse, CreateInvitationRequest, CreateInvitationResponse,
    CreateOrganizationRequest, CreateOrganizationResponse, GetInvitationResponse,
    GetOrganizationResponse, ListInvitationsResponse, ListMembersResponse,
    ListOrganizationsResponse, MemberRole, Organization, OrganizationMemberWithProfile,
    OrganizationWithRole, RevokeInvitationRequest, UpdateMemberRoleRequest,
    UpdateMemberRoleResponse, UpdateOrganizationRequest,
};
use axum::{
    Router,
    extract::{Json, Path, State},
    http::StatusCode,
    response::Json as ResponseJson,
    routing::{delete, get, patch, post},
};
use chrono::Utc;
use deployment::Deployment;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

/// Fixed UUID for the local organization.
const LOCAL_ORG_ID: &str = "00000000-0000-0000-0000-000000000001";
/// Fixed UUID for the local user.
const LOCAL_USER_ID: &str = "00000000-0000-0000-0000-000000000002";

fn local_org() -> Organization {
    let id: Uuid = LOCAL_ORG_ID.parse().unwrap();
    let now = Utc::now();
    Organization {
        id,
        name: "Local".to_string(),
        slug: "local".to_string(),
        is_personal: true,
        issue_prefix: "LOC".to_string(),
        created_at: now,
        updated_at: now,
    }
}

fn local_org_with_role() -> OrganizationWithRole {
    let id: Uuid = LOCAL_ORG_ID.parse().unwrap();
    let now = Utc::now();
    OrganizationWithRole {
        id,
        name: "Local".to_string(),
        slug: "local".to_string(),
        is_personal: true,
        issue_prefix: "LOC".to_string(),
        created_at: now,
        updated_at: now,
        user_role: MemberRole::Admin,
    }
}

pub fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/organizations", get(list_organizations))
        .route("/organizations", post(create_organization))
        .route("/organizations/{id}", get(get_organization))
        .route("/organizations/{id}", patch(update_organization))
        .route("/organizations/{id}", delete(delete_organization))
        .route(
            "/organizations/{org_id}/invitations",
            post(create_invitation),
        )
        .route("/organizations/{org_id}/invitations", get(list_invitations))
        .route(
            "/organizations/{org_id}/invitations/revoke",
            post(revoke_invitation),
        )
        .route("/invitations/{token}", get(get_invitation))
        .route("/invitations/{token}/accept", post(accept_invitation))
        .route("/organizations/{org_id}/members", get(list_members))
        .route(
            "/organizations/{org_id}/members/{user_id}",
            delete(remove_member),
        )
        .route(
            "/organizations/{org_id}/members/{user_id}/role",
            patch(update_member_role),
        )
}

// All handlers return raw JSON (no ApiResponse wrapper) because the frontend's
// handleRemoteResponse() expects the same shape as the remote server returns.

async fn list_organizations(
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ListOrganizationsResponse>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.list_organizations().await?;
            Ok(ResponseJson(response))
        }
        Err(_) => Ok(ResponseJson(ListOrganizationsResponse {
            organizations: vec![local_org_with_role()],
        })),
    }
}

async fn get_organization(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<ResponseJson<GetOrganizationResponse>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => Ok(ResponseJson(client.get_organization(id).await?)),
        Err(_) => Ok(ResponseJson(GetOrganizationResponse {
            organization: local_org(),
            user_role: "admin".to_string(),
        })),
    }
}

async fn create_organization(
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<CreateOrganizationRequest>,
) -> Result<ResponseJson<CreateOrganizationResponse>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.create_organization(&request).await?;
            deployment
                .track_if_analytics_allowed(
                    "organization_created",
                    serde_json::json!({
                        "org_id": response.organization.id.to_string(),
                    }),
                )
                .await;
            Ok(ResponseJson(response))
        }
        Err(_) => Ok(ResponseJson(CreateOrganizationResponse {
            organization: local_org_with_role(),
        })),
    }
}

async fn update_organization(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
    Json(request): Json<UpdateOrganizationRequest>,
) -> Result<ResponseJson<Organization>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => Ok(ResponseJson(
            client.update_organization(id, &request).await?,
        )),
        Err(_) => Ok(ResponseJson(local_org())),
    }
}

async fn delete_organization(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            client.delete_organization(id).await?;
            Ok(StatusCode::NO_CONTENT)
        }
        Err(_) => Ok(StatusCode::NO_CONTENT),
    }
}

async fn create_invitation(
    State(deployment): State<DeploymentImpl>,
    Path(org_id): Path<Uuid>,
    Json(request): Json<CreateInvitationRequest>,
) -> Result<ResponseJson<CreateInvitationResponse>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            let response = client.create_invitation(org_id, &request).await?;
            deployment
                .track_if_analytics_allowed(
                    "invitation_created",
                    serde_json::json!({
                        "invitation_id": response.invitation.id.to_string(),
                        "org_id": org_id.to_string(),
                        "role": response.invitation.role,
                    }),
                )
                .await;
            Ok(ResponseJson(response))
        }
        Err(_) => Err(ApiError::BadRequest(
            "Invitations not available in local mode".into(),
        )),
    }
}

async fn list_invitations(
    State(deployment): State<DeploymentImpl>,
    Path(org_id): Path<Uuid>,
) -> Result<ResponseJson<ListInvitationsResponse>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => Ok(ResponseJson(client.list_invitations(org_id).await?)),
        Err(_) => Ok(ResponseJson(ListInvitationsResponse {
            invitations: vec![],
        })),
    }
}

async fn get_invitation(
    State(deployment): State<DeploymentImpl>,
    Path(token): Path<String>,
) -> Result<ResponseJson<GetInvitationResponse>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => Ok(ResponseJson(client.get_invitation(&token).await?)),
        Err(_) => Err(ApiError::BadRequest(
            "Invitations not available in local mode".into(),
        )),
    }
}

async fn revoke_invitation(
    State(deployment): State<DeploymentImpl>,
    Path(org_id): Path<Uuid>,
    Json(payload): Json<RevokeInvitationRequest>,
) -> Result<StatusCode, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            client
                .revoke_invitation(org_id, payload.invitation_id)
                .await?;
            Ok(StatusCode::NO_CONTENT)
        }
        Err(_) => Ok(StatusCode::NO_CONTENT),
    }
}

async fn accept_invitation(
    State(deployment): State<DeploymentImpl>,
    Path(invitation_token): Path<String>,
) -> Result<ResponseJson<AcceptInvitationResponse>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => Ok(ResponseJson(
            client.accept_invitation(&invitation_token).await?,
        )),
        Err(_) => Err(ApiError::BadRequest(
            "Invitations not available in local mode".into(),
        )),
    }
}

async fn list_members(
    State(deployment): State<DeploymentImpl>,
    Path(org_id): Path<Uuid>,
) -> Result<ResponseJson<ListMembersResponse>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => Ok(ResponseJson(client.list_members(org_id).await?)),
        Err(_) => {
            let local_user_id: Uuid = LOCAL_USER_ID.parse().unwrap();
            Ok(ResponseJson(ListMembersResponse {
                members: vec![OrganizationMemberWithProfile {
                    user_id: local_user_id,
                    role: MemberRole::Admin,
                    joined_at: Utc::now(),
                    first_name: Some("Local".to_string()),
                    last_name: Some("User".to_string()),
                    username: Some("local-user".to_string()),
                    email: None,
                    avatar_url: None,
                }],
            }))
        }
    }
}

async fn remove_member(
    State(deployment): State<DeploymentImpl>,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    match deployment.remote_client() {
        Ok(client) => {
            client.remove_member(org_id, user_id).await?;
            Ok(StatusCode::NO_CONTENT)
        }
        Err(_) => Ok(StatusCode::NO_CONTENT),
    }
}

async fn update_member_role(
    State(deployment): State<DeploymentImpl>,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateMemberRoleRequest>,
) -> Result<ResponseJson<UpdateMemberRoleResponse>, ApiError> {
    match deployment.remote_client() {
        Ok(client) => Ok(ResponseJson(
            client.update_member_role(org_id, user_id, &request).await?,
        )),
        Err(_) => Err(ApiError::BadRequest(
            "Member management not available in local mode".into(),
        )),
    }
}
