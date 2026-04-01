use api_types::project_status::ProjectStatus;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

const STATUS_SELECT: &str = r#"SELECT
    id, project_id, name, color, sort_order, hidden, created_at
FROM project_statuses"#;

fn status_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ProjectStatus, sqlx::Error> {
    Ok(ProjectStatus {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        name: row.try_get("name")?,
        color: row.try_get("color")?,
        sort_order: row.try_get("sort_order")?,
        hidden: row.try_get("hidden")?,
        created_at: row.try_get("created_at")?,
    })
}

pub struct LocalProjectStatus;

impl LocalProjectStatus {
    pub async fn find_by_project(
        pool: &SqlitePool,
        project_id: Uuid,
    ) -> Result<Vec<ProjectStatus>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "{STATUS_SELECT} WHERE project_id = $1 ORDER BY sort_order ASC"
        ))
        .bind(project_id)
        .fetch_all(pool)
        .await?;
        rows.iter().map(status_from_row).collect()
    }

    pub async fn find_by_id(
        pool: &SqlitePool,
        id: Uuid,
    ) -> Result<Option<ProjectStatus>, sqlx::Error> {
        let row = sqlx::query(&format!("{STATUS_SELECT} WHERE id = $1"))
            .bind(id)
            .fetch_optional(pool)
            .await?;
        row.as_ref().map(status_from_row).transpose()
    }

    pub async fn create(
        pool: &SqlitePool,
        project_id: Uuid,
        name: &str,
        color: &str,
        sort_order: i32,
    ) -> Result<ProjectStatus, sqlx::Error> {
        let id = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO project_statuses (id, project_id, name, color, sort_order, hidden, created_at)
             VALUES ($1, $2, $3, $4, $5, false, datetime('now', 'subsec'))",
        )
        .bind(id)
        .bind(project_id)
        .bind(name)
        .bind(color)
        .bind(sort_order)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)
    }

    /// Create default statuses for a project: Todo, In Progress, In Review, Done.
    pub async fn create_defaults(pool: &SqlitePool, project_id: Uuid) -> Result<(), sqlx::Error> {
        let defaults = [
            ("Todo", "#6b7280", 0),        // gray
            ("In Progress", "#3b82f6", 1), // blue
            ("In Review", "#f59e0b", 2),   // amber
            ("Done", "#22c55e", 3),        // green
        ];

        for (name, color, sort_order) in &defaults {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO project_statuses (id, project_id, name, color, sort_order, hidden, created_at)
                 VALUES ($1, $2, $3, $4, $5, false, datetime('now', 'subsec'))",
            )
            .bind(id)
            .bind(project_id)
            .bind(name)
            .bind(color)
            .bind(sort_order)
            .execute(pool)
            .await?;
        }

        Ok(())
    }

    pub async fn update(
        pool: &SqlitePool,
        id: Uuid,
        name: Option<&str>,
        color: Option<&str>,
        sort_order: Option<i32>,
        hidden: Option<bool>,
    ) -> Result<ProjectStatus, sqlx::Error> {
        sqlx::query(
            "UPDATE project_statuses SET
                name = COALESCE($2, name),
                color = COALESCE($3, color),
                sort_order = COALESCE($4, sort_order),
                hidden = COALESCE($5, hidden)
            WHERE id = $1",
        )
        .bind(id)
        .bind(name)
        .bind(color)
        .bind(sort_order)
        .bind(hidden)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)
    }

    pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM project_statuses WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
