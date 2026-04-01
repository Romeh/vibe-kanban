use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueAssignee {
    pub id: Uuid,
    pub issue_id: Uuid,
    pub user_id: Uuid,
    pub assigned_at: DateTime<Utc>,
}

const ISSUE_ASSIGNEE_SELECT: &str =
    r#"SELECT id, issue_id, user_id, assigned_at FROM issue_assignees"#;

fn issue_assignee_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<IssueAssignee, sqlx::Error> {
    Ok(IssueAssignee {
        id: row.try_get("id")?,
        issue_id: row.try_get("issue_id")?,
        user_id: row.try_get("user_id")?,
        assigned_at: row.try_get("assigned_at")?,
    })
}

impl IssueAssignee {
    pub async fn find_by_issue(
        pool: &SqlitePool,
        issue_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        let rows = sqlx::query(&format!("{ISSUE_ASSIGNEE_SELECT} WHERE issue_id = $1"))
            .bind(issue_id)
            .fetch_all(pool)
            .await?;
        rows.iter().map(issue_assignee_from_row).collect()
    }

    pub async fn find_by_project_issues(
        pool: &SqlitePool,
        project_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT ia.id, ia.issue_id, ia.user_id, ia.assigned_at FROM issue_assignees ia \
             INNER JOIN issues ON issues.id = ia.issue_id \
             WHERE issues.project_id = $1",
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?;
        rows.iter().map(issue_assignee_from_row).collect()
    }

    pub async fn create(
        pool: &SqlitePool,
        issue_id: Uuid,
        user_id: Uuid,
    ) -> Result<Self, sqlx::Error> {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO issue_assignees (id, issue_id, user_id) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(issue_id)
            .bind(user_id)
            .execute(pool)
            .await?;

        let row = sqlx::query(&format!("{ISSUE_ASSIGNEE_SELECT} WHERE id = $1"))
            .bind(id)
            .fetch_one(pool)
            .await?;
        issue_assignee_from_row(&row)
    }

    pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM issue_assignees WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
