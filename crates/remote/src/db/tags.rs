use api_types::{DeleteResponse, MutationResponse, Tag};
use sqlx::{Executor, PgPool, Postgres};
use thiserror::Error;
use uuid::Uuid;

use super::get_txid;

#[derive(Debug, Error)]
pub enum TagError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Default tags that are created for each new project
/// Colors are in HSL format: "H S% L%"
pub const DEFAULT_TAGS: &[(&str, &str)] = &[
    ("bug", "355 65% 53%"),
    ("feature", "124 82% 30%"),
    ("documentation", "205 100% 40%"),
    ("enhancement", "181 72% 78%"),
];

/// Internal row type with `FromRow` for runtime queries.
#[derive(sqlx::FromRow)]
struct TagRow {
    id: Uuid,
    project_id: Uuid,
    name: String,
    color: String,
}

impl From<TagRow> for Tag {
    fn from(row: TagRow) -> Self {
        Tag {
            id: row.id,
            project_id: row.project_id,
            name: row.name,
            color: row.color,
        }
    }
}

pub struct TagRepository;

impl TagRepository {
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Tag>, TagError> {
        let record = sqlx::query_as!(
            Tag,
            r#"
            SELECT
                id          AS "id!: Uuid",
                project_id  AS "project_id!: Uuid",
                name        AS "name!",
                color       AS "color!"
            FROM tags
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(pool)
        .await?;

        Ok(record)
    }

    pub async fn create(
        pool: &PgPool,
        id: Option<Uuid>,
        project_id: Uuid,
        name: String,
        color: String,
    ) -> Result<MutationResponse<Tag>, TagError> {
        let mut tx = super::begin_tx(pool).await?;

        let id = id.unwrap_or_else(Uuid::new_v4);
        let data = sqlx::query_as!(
            Tag,
            r#"
            INSERT INTO tags (id, project_id, name, color)
            VALUES ($1, $2, $3, $4)
            RETURNING
                id          AS "id!: Uuid",
                project_id  AS "project_id!: Uuid",
                name        AS "name!",
                color       AS "color!"
            "#,
            id,
            project_id,
            name,
            color
        )
        .fetch_one(&mut *tx)
        .await?;

        let txid = get_txid(&mut *tx).await?;
        tx.commit().await?;

        Ok(MutationResponse { data, txid })
    }

    /// Update a tag with partial fields. Uses COALESCE to preserve existing values
    /// when None is provided.
    pub async fn update(
        pool: &PgPool,
        id: Uuid,
        name: Option<String>,
        color: Option<String>,
    ) -> Result<MutationResponse<Tag>, TagError> {
        let mut tx = super::begin_tx(pool).await?;

        let data = sqlx::query_as!(
            Tag,
            r#"
            UPDATE tags
            SET
                name = COALESCE($1, name),
                color = COALESCE($2, color)
            WHERE id = $3
            RETURNING
                id          AS "id!: Uuid",
                project_id  AS "project_id!: Uuid",
                name        AS "name!",
                color       AS "color!"
            "#,
            name,
            color,
            id
        )
        .fetch_one(&mut *tx)
        .await?;

        let txid = get_txid(&mut *tx).await?;
        tx.commit().await?;

        Ok(MutationResponse { data, txid })
    }

    pub async fn delete(pool: &PgPool, id: Uuid) -> Result<DeleteResponse, TagError> {
        let mut tx = super::begin_tx(pool).await?;

        sqlx::query!("DELETE FROM tags WHERE id = $1", id)
            .execute(&mut *tx)
            .await?;

        let txid = get_txid(&mut *tx).await?;
        tx.commit().await?;

        Ok(DeleteResponse { txid })
    }

    /// Find an existing tag by name (case-insensitive) or create one with a default color.
    /// Used during Jira import to map Jira labels → VK tags.
    pub async fn find_or_create_by_name(
        pool: &PgPool,
        project_id: Uuid,
        name: &str,
    ) -> Result<Tag, TagError> {
        // Try to find existing tag (case-insensitive)
        let existing: Option<TagRow> = sqlx::query_as(
            "SELECT id, project_id, name, color FROM tags WHERE project_id = $1 AND LOWER(name) = LOWER($2)",
        )
        .bind(project_id)
        .bind(name)
        .fetch_optional(pool)
        .await?;

        if let Some(row) = existing {
            return Ok(row.into());
        }

        // Create new tag with a neutral gray color
        let id = Uuid::new_v4();
        let row: TagRow = sqlx::query_as(
            "INSERT INTO tags (id, project_id, name, color) VALUES ($1, $2, $3, $4) RETURNING id, project_id, name, color",
        )
        .bind(id)
        .bind(project_id)
        .bind(name)
        .bind("210 10% 58%")
        .fetch_one(pool)
        .await?;

        Ok(row.into())
    }

    pub async fn list_by_project(pool: &PgPool, project_id: Uuid) -> Result<Vec<Tag>, TagError> {
        let records = sqlx::query_as!(
            Tag,
            r#"
            SELECT
                id          AS "id!: Uuid",
                project_id  AS "project_id!: Uuid",
                name        AS "name!",
                color       AS "color!"
            FROM tags
            WHERE project_id = $1
            "#,
            project_id
        )
        .fetch_all(pool)
        .await?;

        Ok(records)
    }

    pub async fn create_default_tags<'e, E>(
        executor: E,
        project_id: Uuid,
    ) -> Result<Vec<Tag>, TagError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let names: Vec<String> = DEFAULT_TAGS.iter().map(|(n, _)| (*n).to_string()).collect();
        let colors: Vec<String> = DEFAULT_TAGS.iter().map(|(_, c)| (*c).to_string()).collect();

        let tags = sqlx::query_as!(
            Tag,
            r#"
            INSERT INTO tags (id, project_id, name, color)
            SELECT gen_random_uuid(), $1, name, color
            FROM UNNEST($2::text[], $3::text[]) AS t(name, color)
            RETURNING
                id          AS "id!: Uuid",
                project_id  AS "project_id!: Uuid",
                name        AS "name!",
                color       AS "color!"
            "#,
            project_id,
            &names,
            &colors
        )
        .fetch_all(executor)
        .await?;

        Ok(tags)
    }
}
