use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraConnectionRow {
    pub id: Uuid,
    pub jira_site_url: String,
    pub auth_type: String,
    pub encrypted_credentials: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusMappingRow {
    pub vk_status_name: String,
    pub jira_category_key: String,
}

impl JiraConnectionRow {
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get::<Uuid, _>("id")?,
            jira_site_url: row.try_get("jira_site_url")?,
            auth_type: row.try_get("auth_type")?,
            encrypted_credentials: row.try_get("encrypted_credentials")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }

    pub async fn find(pool: &SqlitePool) -> Result<Option<Self>, sqlx::Error> {
        let row = sqlx::query("SELECT * FROM jira_connections LIMIT 1")
            .fetch_optional(pool)
            .await?;
        row.as_ref().map(Self::from_row).transpose()
    }

    pub async fn upsert(
        pool: &SqlitePool,
        site_url: &str,
        auth_type: &str,
        encrypted_credentials: &[u8],
    ) -> Result<Self, sqlx::Error> {
        // Delete existing (single-user, max one row).
        sqlx::query("DELETE FROM jira_connections")
            .execute(pool)
            .await?;

        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO jira_connections (id, jira_site_url, auth_type, encrypted_credentials) VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(site_url)
        .bind(auth_type)
        .bind(encrypted_credentials)
        .execute(pool)
        .await?;

        Self::find(pool).await?.ok_or(sqlx::Error::RowNotFound)
    }

    pub async fn update_credentials(
        pool: &SqlitePool,
        encrypted_credentials: &[u8],
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE jira_connections SET encrypted_credentials = $1, updated_at = datetime('now', 'subsec')",
        )
        .bind(encrypted_credentials)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn delete(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM jira_connections")
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

impl StatusMappingRow {
    pub async fn find_all(pool: &SqlitePool) -> Result<Vec<Self>, sqlx::Error> {
        let rows =
            sqlx::query("SELECT vk_status_name, jira_category_key FROM jira_status_mappings")
                .fetch_all(pool)
                .await?;

        rows.iter()
            .map(|r| {
                Ok(Self {
                    vk_status_name: r.try_get("vk_status_name")?,
                    jira_category_key: r.try_get("jira_category_key")?,
                })
            })
            .collect()
    }

    pub async fn upsert(
        pool: &SqlitePool,
        vk_status_name: &str,
        jira_category_key: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO jira_status_mappings (id, vk_status_name, jira_category_key) VALUES (randomblob(16), $1, $2) ON CONFLICT (vk_status_name) DO UPDATE SET jira_category_key = $2",
        )
        .bind(vk_status_name)
        .bind(jira_category_key)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn delete(pool: &SqlitePool, vk_status_name: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM jira_status_mappings WHERE vk_status_name = $1")
            .bind(vk_status_name)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
