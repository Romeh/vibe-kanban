use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTag {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub color: String,
}

const PROJECT_TAG_SELECT: &str = r#"SELECT id, project_id, name, color FROM project_tags"#;

fn project_tag_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ProjectTag, sqlx::Error> {
    Ok(ProjectTag {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        name: row.try_get("name")?,
        color: row.try_get("color")?,
    })
}

impl ProjectTag {
    pub async fn find_by_project(
        pool: &SqlitePool,
        project_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "{PROJECT_TAG_SELECT} WHERE project_id = $1 ORDER BY name ASC"
        ))
        .bind(project_id)
        .fetch_all(pool)
        .await?;
        rows.iter().map(project_tag_from_row).collect()
    }

    pub async fn find_or_create_by_name(
        pool: &SqlitePool,
        project_id: Uuid,
        name: &str,
    ) -> Result<Self, sqlx::Error> {
        let row = sqlx::query(&format!(
            "{PROJECT_TAG_SELECT} WHERE project_id = $1 AND name = $2"
        ))
        .bind(project_id)
        .bind(name)
        .fetch_optional(pool)
        .await?;

        if let Some(ref row) = row {
            return project_tag_from_row(row);
        }

        // Default color for auto-created tags
        Self::create(pool, project_id, name, "#6B7280").await
    }

    pub async fn create(
        pool: &SqlitePool,
        project_id: Uuid,
        name: &str,
        color: &str,
    ) -> Result<Self, sqlx::Error> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO project_tags (id, project_id, name, color) VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(project_id)
        .bind(name)
        .bind(color)
        .execute(pool)
        .await?;

        let row = sqlx::query(&format!("{PROJECT_TAG_SELECT} WHERE id = $1"))
            .bind(id)
            .fetch_one(pool)
            .await?;
        project_tag_from_row(&row)
    }

    pub async fn update(
        pool: &SqlitePool,
        id: Uuid,
        name: &str,
        color: &str,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query("UPDATE project_tags SET name = $2, color = $3 WHERE id = $1")
            .bind(id)
            .bind(name)
            .bind(color)
            .execute(pool)
            .await?;

        let row = sqlx::query(&format!("{PROJECT_TAG_SELECT} WHERE id = $1"))
            .bind(id)
            .fetch_one(pool)
            .await?;
        project_tag_from_row(&row)
    }

    pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM project_tags WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
