use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub default_agent_working_dir: Option<String>,
    pub remote_project_id: Option<Uuid>,
    #[ts(type = "Date")]
    pub created_at: DateTime<Utc>,
    #[ts(type = "Date")]
    pub updated_at: DateTime<Utc>,
    pub organization_id: Option<Uuid>,
    pub identifier: Option<String>,
    pub color: Option<String>,
    pub sort_order: Option<i32>,
}

fn project_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Project, sqlx::Error> {
    Ok(Project {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        default_agent_working_dir: row.try_get("default_agent_working_dir")?,
        remote_project_id: row.try_get("remote_project_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        organization_id: row.try_get("organization_id").ok(),
        identifier: row.try_get("identifier").ok(),
        color: row.try_get("color").ok(),
        sort_order: row.try_get("sort_order").ok(),
    })
}

impl Project {
    pub async fn find_all(pool: &SqlitePool) -> Result<Vec<Self>, sqlx::Error> {
        let rows = sqlx::query("SELECT * FROM projects ORDER BY created_at DESC")
            .fetch_all(pool)
            .await?;
        rows.iter().map(project_from_row).collect()
    }

    pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        let row = sqlx::query("SELECT * FROM projects WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
        row.as_ref().map(project_from_row).transpose()
    }

    pub async fn set_remote_project_id(
        pool: &SqlitePool,
        id: Uuid,
        remote_project_id: Option<Uuid>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE projects SET remote_project_id = $2 WHERE id = $1")
            .bind(id)
            .bind(remote_project_id)
            .execute(pool)
            .await?;
        Ok(())
    }
}
