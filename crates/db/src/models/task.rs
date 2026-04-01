use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, Row, SqlitePool, Type};
use strum_macros::{Display, EnumString};
use ts_rs::TS;
use uuid::Uuid;

#[derive(
    Debug, Clone, Type, Serialize, Deserialize, PartialEq, TS, EnumString, Display, Default,
)]
#[sqlx(type_name = "task_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum TaskStatus {
    #[default]
    Todo,
    InProgress,
    InReview,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, TS)]
pub struct Task {
    pub id: Uuid,
    pub project_id: Uuid, // Foreign key to Project
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub parent_workspace_id: Option<Uuid>, // Foreign key to parent Workspace
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[sqlx(default)]
    #[ts(type = "Record<string, unknown>")]
    pub extension_metadata: Value,
    #[sqlx(default)]
    pub priority: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTaskPayload {
    pub project_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub extension_metadata: Option<Value>,
    pub priority: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTaskPayload {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<TaskStatus>,
    pub extension_metadata: Option<Value>,
    pub priority: Option<String>,
}

const TASK_SELECT: &str = r#"SELECT id, project_id, title, description, status, parent_workspace_id, created_at, updated_at, extension_metadata, priority FROM tasks"#;

fn task_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Task, sqlx::Error> {
    let ext_str: String = row
        .try_get("extension_metadata")
        .unwrap_or_else(|_| "{}".to_string());
    let extension_metadata: Value =
        serde_json::from_str(&ext_str).unwrap_or_else(|_| Value::Object(Default::default()));
    Ok(Task {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        status: row.try_get("status")?,
        parent_workspace_id: row.try_get("parent_workspace_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        extension_metadata,
        priority: row.try_get("priority")?,
    })
}

impl Task {
    pub async fn find_all(pool: &SqlitePool) -> Result<Vec<Self>, sqlx::Error> {
        let rows = sqlx::query(&format!("{TASK_SELECT} ORDER BY created_at ASC"))
            .fetch_all(pool)
            .await?;
        rows.iter().map(task_from_row).collect()
    }

    pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        let row = sqlx::query(&format!("{TASK_SELECT} WHERE id = $1"))
            .bind(id)
            .fetch_optional(pool)
            .await?;
        row.as_ref().map(task_from_row).transpose()
    }

    pub async fn find_by_project_id(
        pool: &SqlitePool,
        project_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "{TASK_SELECT} WHERE project_id = $1 ORDER BY created_at ASC"
        ))
        .bind(project_id)
        .fetch_all(pool)
        .await?;
        rows.iter().map(task_from_row).collect()
    }

    pub async fn find_by_jira_key(
        pool: &SqlitePool,
        project_id: Uuid,
        jira_key: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        let row = sqlx::query(&format!(
            "{TASK_SELECT} WHERE project_id = $1 AND json_extract(extension_metadata, '$.jira.issue_key') = $2"
        ))
        .bind(project_id)
        .bind(jira_key)
        .fetch_optional(pool)
        .await?;
        row.as_ref().map(task_from_row).transpose()
    }

    pub async fn create(
        pool: &SqlitePool,
        payload: &CreateTaskPayload,
    ) -> Result<Self, sqlx::Error> {
        let id = Uuid::new_v4();
        let status = payload
            .status
            .as_ref()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "todo".to_string());
        let extension_metadata = payload
            .extension_metadata
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "{}".to_string());

        sqlx::query(
            "INSERT INTO tasks (id, project_id, title, description, status, extension_metadata, priority) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(id)
        .bind(payload.project_id)
        .bind(&payload.title)
        .bind(&payload.description)
        .bind(&status)
        .bind(&extension_metadata)
        .bind(&payload.priority)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)
    }

    pub async fn update(
        pool: &SqlitePool,
        id: Uuid,
        payload: &UpdateTaskPayload,
    ) -> Result<Self, sqlx::Error> {
        let status = payload.status.as_ref().map(|s| s.to_string());
        let extension_metadata = payload.extension_metadata.as_ref().map(|v| v.to_string());

        sqlx::query(
            "UPDATE tasks SET title = COALESCE($2, title), description = COALESCE($3, description), status = COALESCE($4, status), extension_metadata = COALESCE($5, extension_metadata), priority = COALESCE($6, priority), updated_at = datetime('now', 'subsec') WHERE id = $1",
        )
        .bind(id)
        .bind(&payload.title)
        .bind(&payload.description)
        .bind(&status)
        .bind(&extension_metadata)
        .bind(&payload.priority)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)
    }
}
