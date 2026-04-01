use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueTag {
    pub id: Uuid,
    pub issue_id: Uuid,
    pub tag_id: Uuid,
}

const ISSUE_TAG_SELECT: &str = r#"SELECT id, issue_id, tag_id FROM issue_tags"#;

fn issue_tag_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<IssueTag, sqlx::Error> {
    Ok(IssueTag {
        id: row.try_get("id")?,
        issue_id: row.try_get("issue_id")?,
        tag_id: row.try_get("tag_id")?,
    })
}

impl IssueTag {
    pub async fn find_by_issue(
        pool: &SqlitePool,
        issue_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        let rows = sqlx::query(&format!("{ISSUE_TAG_SELECT} WHERE issue_id = $1"))
            .bind(issue_id)
            .fetch_all(pool)
            .await?;
        rows.iter().map(issue_tag_from_row).collect()
    }

    pub async fn find_by_project_issues(
        pool: &SqlitePool,
        project_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT it.id, it.issue_id, it.tag_id FROM issue_tags it \
             INNER JOIN issues ON issues.id = it.issue_id \
             WHERE issues.project_id = $1",
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?;
        rows.iter().map(issue_tag_from_row).collect()
    }

    pub async fn create(
        pool: &SqlitePool,
        issue_id: Uuid,
        tag_id: Uuid,
    ) -> Result<Self, sqlx::Error> {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO issue_tags (id, issue_id, tag_id) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(issue_id)
            .bind(tag_id)
            .execute(pool)
            .await?;

        let row = sqlx::query(&format!("{ISSUE_TAG_SELECT} WHERE id = $1"))
            .bind(id)
            .fetch_one(pool)
            .await?;
        issue_tag_from_row(&row)
    }

    pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM issue_tags WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
