use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueComment {
    pub id: Uuid,
    pub issue_id: Uuid,
    pub author_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub message: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const ISSUE_COMMENT_SELECT: &str = r#"SELECT id, issue_id, author_id, parent_id, message, created_at, updated_at FROM issue_comments"#;

fn issue_comment_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<IssueComment, sqlx::Error> {
    Ok(IssueComment {
        id: row.try_get("id")?,
        issue_id: row.try_get("issue_id")?,
        author_id: row.try_get("author_id")?,
        parent_id: row.try_get("parent_id")?,
        message: row.try_get("message")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

impl IssueComment {
    pub async fn find_by_issue(
        pool: &SqlitePool,
        issue_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "{ISSUE_COMMENT_SELECT} WHERE issue_id = $1 ORDER BY created_at ASC"
        ))
        .bind(issue_id)
        .fetch_all(pool)
        .await?;
        rows.iter().map(issue_comment_from_row).collect()
    }

    pub async fn create(
        pool: &SqlitePool,
        issue_id: Uuid,
        author_id: Option<Uuid>,
        parent_id: Option<Uuid>,
        message: &str,
    ) -> Result<Self, sqlx::Error> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO issue_comments (id, issue_id, author_id, parent_id, message) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(issue_id)
        .bind(author_id)
        .bind(parent_id)
        .bind(message)
        .execute(pool)
        .await?;

        let row = sqlx::query(&format!("{ISSUE_COMMENT_SELECT} WHERE id = $1"))
            .bind(id)
            .fetch_one(pool)
            .await?;
        issue_comment_from_row(&row)
    }

    pub async fn update(pool: &SqlitePool, id: Uuid, message: &str) -> Result<Self, sqlx::Error> {
        sqlx::query(
            "UPDATE issue_comments SET message = $2, updated_at = datetime('now', 'subsec') \
             WHERE id = $1",
        )
        .bind(id)
        .bind(message)
        .execute(pool)
        .await?;

        let row = sqlx::query(&format!("{ISSUE_COMMENT_SELECT} WHERE id = $1"))
            .bind(id)
            .fetch_one(pool)
            .await?;
        issue_comment_from_row(&row)
    }

    pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM issue_comments WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
