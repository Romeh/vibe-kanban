use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
pub struct JiraConnectionRow {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub jira_site_url: String,
    pub auth_type: String,
    pub encrypted_credentials: Vec<u8>,
    pub connected_by_user_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct JiraRepository;

impl JiraRepository {
    pub async fn find_by_org(
        pool: &PgPool,
        organization_id: Uuid,
    ) -> Result<Option<JiraConnectionRow>, sqlx::Error> {
        sqlx::query_as::<_, JiraConnectionRow>(
            r#"
            SELECT id, organization_id, jira_site_url, auth_type,
                   encrypted_credentials, connected_by_user_id,
                   created_at, updated_at
            FROM jira_connections
            WHERE organization_id = $1
            "#,
        )
        .bind(organization_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn upsert(
        pool: &PgPool,
        organization_id: Uuid,
        jira_site_url: &str,
        auth_type: &str,
        encrypted_credentials: &[u8],
        connected_by_user_id: Uuid,
    ) -> Result<JiraConnectionRow, sqlx::Error> {
        sqlx::query_as::<_, JiraConnectionRow>(
            r#"
            INSERT INTO jira_connections
                (organization_id, jira_site_url, auth_type, encrypted_credentials, connected_by_user_id)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (organization_id) DO UPDATE SET
                jira_site_url = EXCLUDED.jira_site_url,
                auth_type = EXCLUDED.auth_type,
                encrypted_credentials = EXCLUDED.encrypted_credentials,
                connected_by_user_id = EXCLUDED.connected_by_user_id,
                updated_at = NOW()
            RETURNING id, organization_id, jira_site_url, auth_type,
                      encrypted_credentials, connected_by_user_id,
                      created_at, updated_at
            "#,
        )
        .bind(organization_id)
        .bind(jira_site_url)
        .bind(auth_type)
        .bind(encrypted_credentials)
        .bind(connected_by_user_id)
        .fetch_one(pool)
        .await
    }

    pub async fn update_credentials(
        pool: &PgPool,
        organization_id: Uuid,
        encrypted_credentials: &[u8],
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE jira_connections
            SET encrypted_credentials = $1, updated_at = NOW()
            WHERE organization_id = $2
            "#,
        )
        .bind(encrypted_credentials)
        .bind(organization_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn delete_by_org(pool: &PgPool, organization_id: Uuid) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM jira_connections WHERE organization_id = $1")
            .bind(organization_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // -----------------------------------------------------------------------
    // Status mappings
    // -----------------------------------------------------------------------

    pub async fn get_status_mappings(
        pool: &PgPool,
        organization_id: Uuid,
    ) -> Result<Vec<StatusMappingRow>, sqlx::Error> {
        sqlx::query_as::<_, StatusMappingRow>(
            "SELECT vk_status_name, jira_category_key FROM jira_status_mappings WHERE organization_id = $1",
        )
        .bind(organization_id)
        .fetch_all(pool)
        .await
    }

    pub async fn upsert_status_mapping(
        pool: &PgPool,
        organization_id: Uuid,
        vk_status_name: &str,
        jira_category_key: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO jira_status_mappings (organization_id, vk_status_name, jira_category_key)
            VALUES ($1, $2, $3)
            ON CONFLICT (organization_id, vk_status_name) DO UPDATE SET
                jira_category_key = EXCLUDED.jira_category_key
            "#,
        )
        .bind(organization_id)
        .bind(vk_status_name)
        .bind(jira_category_key)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn delete_status_mapping(
        pool: &PgPool,
        organization_id: Uuid,
        vk_status_name: &str,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "DELETE FROM jira_status_mappings WHERE organization_id = $1 AND vk_status_name = $2",
        )
        .bind(organization_id)
        .bind(vk_status_name)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct StatusMappingRow {
    pub vk_status_name: String,
    pub jira_category_key: String,
}
