use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use strum_macros::{Display, EnumString};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum IssueRelationshipType {
    Blocking,
    Related,
    HasDuplicate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueRelationship {
    pub id: Uuid,
    pub issue_id: Uuid,
    pub related_issue_id: Uuid,
    pub relationship_type: IssueRelationshipType,
    pub created_at: DateTime<Utc>,
}

const ISSUE_RELATIONSHIP_SELECT: &str = r#"SELECT id, issue_id, related_issue_id, relationship_type, created_at FROM issue_relationships"#;

fn issue_relationship_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<IssueRelationship, sqlx::Error> {
    let relationship_type_str: String = row.try_get("relationship_type")?;
    let relationship_type: IssueRelationshipType =
        relationship_type_str
            .parse()
            .map_err(|e| sqlx::Error::ColumnDecode {
                index: "relationship_type".to_string(),
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid relationship_type: {e}"),
                )),
            })?;

    Ok(IssueRelationship {
        id: row.try_get("id")?,
        issue_id: row.try_get("issue_id")?,
        related_issue_id: row.try_get("related_issue_id")?,
        relationship_type,
        created_at: row.try_get("created_at")?,
    })
}

impl IssueRelationship {
    pub async fn find_by_issue(
        pool: &SqlitePool,
        issue_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "{ISSUE_RELATIONSHIP_SELECT} WHERE issue_id = $1 OR related_issue_id = $1"
        ))
        .bind(issue_id)
        .fetch_all(pool)
        .await?;
        rows.iter().map(issue_relationship_from_row).collect()
    }

    pub async fn find_by_project_issues(
        pool: &SqlitePool,
        project_id: Uuid,
    ) -> Result<Vec<Self>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT ir.id, ir.issue_id, ir.related_issue_id, ir.relationship_type, ir.created_at \
             FROM issue_relationships ir \
             INNER JOIN issues ON issues.id = ir.issue_id \
             WHERE issues.project_id = $1",
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?;
        rows.iter().map(issue_relationship_from_row).collect()
    }

    pub async fn create(
        pool: &SqlitePool,
        issue_id: Uuid,
        related_issue_id: Uuid,
        relationship_type: IssueRelationshipType,
    ) -> Result<Self, sqlx::Error> {
        let id = Uuid::new_v4();
        let relationship_type_str = relationship_type.to_string();
        sqlx::query(
            "INSERT INTO issue_relationships (id, issue_id, related_issue_id, relationship_type) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(issue_id)
        .bind(related_issue_id)
        .bind(&relationship_type_str)
        .execute(pool)
        .await?;

        let row = sqlx::query(&format!("{ISSUE_RELATIONSHIP_SELECT} WHERE id = $1"))
            .bind(id)
            .fetch_one(pool)
            .await?;
        issue_relationship_from_row(&row)
    }

    pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM issue_relationships WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
