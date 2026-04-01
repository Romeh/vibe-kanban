use api_types::issue::{
    CreateIssueRequest, Issue, IssuePriority, ListIssuesResponse, SearchIssuesRequest,
    UpdateIssueRequest,
};
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

const ISSUE_SELECT: &str = r#"SELECT
    id, project_id, issue_number, simple_id, status_id, title, description,
    priority, start_date, target_date, completed_at, sort_order,
    parent_issue_id, parent_issue_sort_order, extension_metadata,
    creator_user_id, created_at, updated_at
FROM issues"#;

fn parse_priority(s: Option<String>) -> Option<IssuePriority> {
    s.and_then(|v| match v.as_str() {
        "urgent" => Some(IssuePriority::Urgent),
        "high" => Some(IssuePriority::High),
        "medium" => Some(IssuePriority::Medium),
        "low" => Some(IssuePriority::Low),
        _ => None,
    })
}

fn priority_to_str(p: &IssuePriority) -> &'static str {
    match p {
        IssuePriority::Urgent => "urgent",
        IssuePriority::High => "high",
        IssuePriority::Medium => "medium",
        IssuePriority::Low => "low",
    }
}

fn issue_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Issue, sqlx::Error> {
    let ext_str: String = match row.try_get("extension_metadata") {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                ?e,
                "extension_metadata column missing or unreadable, defaulting to empty"
            );
            "{}".to_string()
        }
    };
    let extension_metadata: Value = match serde_json::from_str(&ext_str) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(?e, ext_str = %ext_str, "corrupt extension_metadata JSON, defaulting to empty object");
            Value::Object(Default::default())
        }
    };
    let priority_str: Option<String> = row.try_get("priority")?;

    Ok(Issue {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        issue_number: row.try_get("issue_number")?,
        simple_id: row.try_get("simple_id")?,
        status_id: row.try_get("status_id")?,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        priority: parse_priority(priority_str),
        start_date: row.try_get("start_date")?,
        target_date: row.try_get("target_date")?,
        completed_at: row.try_get("completed_at")?,
        sort_order: row.try_get("sort_order")?,
        parent_issue_id: row.try_get("parent_issue_id")?,
        parent_issue_sort_order: row.try_get("parent_issue_sort_order")?,
        extension_metadata,
        creator_user_id: row.try_get("creator_user_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub struct LocalIssue;

impl LocalIssue {
    pub async fn find_by_project(
        pool: &SqlitePool,
        project_id: Uuid,
    ) -> Result<Vec<Issue>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "{ISSUE_SELECT} WHERE project_id = $1 ORDER BY sort_order ASC, created_at ASC"
        ))
        .bind(project_id)
        .fetch_all(pool)
        .await?;
        rows.iter().map(issue_from_row).collect()
    }

    pub async fn find_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<Issue>, sqlx::Error> {
        let row = sqlx::query(&format!("{ISSUE_SELECT} WHERE id = $1"))
            .bind(id)
            .fetch_optional(pool)
            .await?;
        row.as_ref().map(issue_from_row).transpose()
    }

    pub async fn create(pool: &SqlitePool, req: &CreateIssueRequest) -> Result<Issue, sqlx::Error> {
        let id = req.id.unwrap_or_else(Uuid::new_v4);

        // Auto-generate issue_number via issue_sequences table inside a transaction
        let mut tx = pool.begin().await?;

        sqlx::query(
            "INSERT OR IGNORE INTO issue_sequences (project_id, next_number) VALUES ($1, 1)",
        )
        .bind(req.project_id)
        .execute(&mut *tx)
        .await?;

        let seq_row = sqlx::query(
            "UPDATE issue_sequences SET next_number = next_number + 1 WHERE project_id = $1 RETURNING next_number - 1 AS issue_number",
        )
        .bind(req.project_id)
        .fetch_one(&mut *tx)
        .await?;

        let issue_number: i64 = seq_row.try_get("issue_number")?;
        let issue_number = issue_number as i32;

        // Build simple_id from project name prefix
        let project_row = sqlx::query("SELECT name FROM projects WHERE id = $1")
            .bind(req.project_id)
            .fetch_optional(&mut *tx)
            .await?;

        let identifier = match project_row {
            Some(row) => {
                let name: String = row.try_get("name")?;
                name.chars()
                    .filter(|c| c.is_alphanumeric())
                    .take(3)
                    .collect::<String>()
                    .to_uppercase()
            }
            None => "PRJ".to_string(),
        };

        let simple_id = format!("{}-{}", identifier, issue_number);

        let priority_str = req.priority.as_ref().map(priority_to_str);
        let extension_metadata_str = req.extension_metadata.to_string();

        sqlx::query(
            "INSERT INTO issues (
                id, project_id, issue_number, simple_id, status_id, title, description,
                priority, start_date, target_date, completed_at, sort_order,
                parent_issue_id, parent_issue_sort_order, extension_metadata,
                creator_user_id, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, NULL,
                datetime('now', 'subsec'), datetime('now', 'subsec')
            )",
        )
        .bind(id)
        .bind(req.project_id)
        .bind(issue_number)
        .bind(&simple_id)
        .bind(req.status_id)
        .bind(&req.title)
        .bind(&req.description)
        .bind(priority_str)
        .bind(req.start_date)
        .bind(req.target_date)
        .bind(req.completed_at)
        .bind(req.sort_order)
        .bind(req.parent_issue_id)
        .bind(req.parent_issue_sort_order)
        .bind(&extension_metadata_str)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Self::find_by_id(pool, id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)
    }

    pub async fn update(
        pool: &SqlitePool,
        id: Uuid,
        req: &UpdateIssueRequest,
    ) -> Result<Issue, sqlx::Error> {
        let status_id = req.status_id;
        let title = req.title.clone();
        // For Option<Option<T>> fields, flatten for COALESCE: None means "don't change",
        // Some(None) means "set to null", Some(Some(v)) means "set to v".
        // We handle this by checking the outer Option and using a sentinel approach.
        let description = req.description.clone().map(|v| v);
        let priority = req
            .priority
            .as_ref()
            .map(|opt| opt.as_ref().map(priority_to_str));
        let start_date = req.start_date;
        let target_date = req.target_date;
        let completed_at = req.completed_at;
        let sort_order = req.sort_order;
        let parent_issue_id = req.parent_issue_id;
        let parent_issue_sort_order = req.parent_issue_sort_order;
        let extension_metadata = req.extension_metadata.as_ref().map(|v| v.to_string());

        sqlx::query(
            "UPDATE issues SET
                status_id = COALESCE($2, status_id),
                title = COALESCE($3, title),
                description = CASE WHEN $4 IS NOT NULL THEN $4 ELSE CASE WHEN $5 THEN NULL ELSE description END END,
                priority = CASE WHEN $6 IS NOT NULL THEN $6 ELSE CASE WHEN $7 THEN NULL ELSE priority END END,
                start_date = CASE WHEN $8 IS NOT NULL THEN $8 ELSE CASE WHEN $9 THEN NULL ELSE start_date END END,
                target_date = CASE WHEN $10 IS NOT NULL THEN $10 ELSE CASE WHEN $11 THEN NULL ELSE target_date END END,
                completed_at = CASE WHEN $12 IS NOT NULL THEN $12 ELSE CASE WHEN $13 THEN NULL ELSE completed_at END END,
                sort_order = COALESCE($14, sort_order),
                parent_issue_id = CASE WHEN $15 IS NOT NULL THEN $15 ELSE CASE WHEN $16 THEN NULL ELSE parent_issue_id END END,
                parent_issue_sort_order = CASE WHEN $17 IS NOT NULL THEN $17 ELSE CASE WHEN $18 THEN NULL ELSE parent_issue_sort_order END END,
                extension_metadata = COALESCE($19, extension_metadata),
                updated_at = datetime('now', 'subsec')
            WHERE id = $1",
        )
        .bind(id)
        // status_id: Option<Uuid>
        .bind(status_id)
        // title: Option<String>
        .bind(&title)
        // description: Option<Option<String>> -> value if Some(Some(v)), flag if Some(None)
        .bind(description.as_ref().and_then(|v| v.as_deref()))
        .bind(description.as_ref().map(|v| v.is_none()).unwrap_or(false))
        // priority: Option<Option<IssuePriority>> -> value if Some(Some(v)), flag if Some(None)
        .bind(priority.as_ref().and_then(|v| *v))
        .bind(priority.as_ref().map(|v| v.is_none()).unwrap_or(false))
        // start_date
        .bind(start_date.and_then(|v| v))
        .bind(start_date.map(|v| v.is_none()).unwrap_or(false))
        // target_date
        .bind(target_date.and_then(|v| v))
        .bind(target_date.map(|v| v.is_none()).unwrap_or(false))
        // completed_at
        .bind(completed_at.and_then(|v| v))
        .bind(completed_at.map(|v| v.is_none()).unwrap_or(false))
        // sort_order
        .bind(sort_order)
        // parent_issue_id
        .bind(parent_issue_id.and_then(|v| v))
        .bind(parent_issue_id.map(|v| v.is_none()).unwrap_or(false))
        // parent_issue_sort_order
        .bind(parent_issue_sort_order.and_then(|v| v))
        .bind(parent_issue_sort_order.map(|v| v.is_none()).unwrap_or(false))
        // extension_metadata
        .bind(&extension_metadata)
        .execute(pool)
        .await?;

        Self::find_by_id(pool, id)
            .await?
            .ok_or(sqlx::Error::RowNotFound)
    }

    pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM issues WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn search(
        pool: &SqlitePool,
        req: &SearchIssuesRequest,
    ) -> Result<ListIssuesResponse, sqlx::Error> {
        let mut where_clauses = vec!["project_id = $1".to_string()];

        // Build dynamic SQL with parameter placeholders bound in order.
        // Track the next bind index (starting from $2 since $1 is project_id).
        let mut bind_idx = 2u32;

        if req.status_id.is_some() {
            where_clauses.push(format!("status_id = ${bind_idx}"));
            bind_idx += 1;
        }

        if let Some(ref status_ids) = req.status_ids {
            if !status_ids.is_empty() {
                let placeholders: Vec<String> = status_ids
                    .iter()
                    .map(|_| {
                        let p = format!("${bind_idx}");
                        bind_idx += 1;
                        p
                    })
                    .collect();
                where_clauses.push(format!("status_id IN ({})", placeholders.join(", ")));
            }
        }

        if req.priority.is_some() {
            where_clauses.push(format!("priority = ${bind_idx}"));
            bind_idx += 1;
        }

        if req.parent_issue_id.is_some() {
            where_clauses.push(format!("parent_issue_id = ${bind_idx}"));
            bind_idx += 1;
        }

        if req.search.is_some() {
            where_clauses.push(format!(
                "(title LIKE ${bind_idx} OR description LIKE ${bind_idx})"
            ));
            bind_idx += 1;
        }

        if req.simple_id.is_some() {
            where_clauses.push(format!("simple_id = ${bind_idx}"));
            #[allow(unused_assignments)]
            {
                bind_idx += 1;
            }
        }

        let where_sql = where_clauses.join(" AND ");

        let sort_field = match req.sort_field {
            Some(api_types::issue::IssueSortField::Priority) => "priority",
            Some(api_types::issue::IssueSortField::CreatedAt) => "created_at",
            Some(api_types::issue::IssueSortField::UpdatedAt) => "updated_at",
            Some(api_types::issue::IssueSortField::Title) => "title",
            _ => "sort_order",
        };

        let sort_dir = match req.sort_direction {
            Some(api_types::issue::SortDirection::Desc) => "DESC",
            _ => "ASC",
        };

        let limit = req.limit.unwrap_or(100) as i64;
        let offset = req.offset.unwrap_or(0) as i64;

        let count_sql = format!("SELECT COUNT(*) as cnt FROM issues WHERE {where_sql}");
        let data_sql = format!(
            "{ISSUE_SELECT} WHERE {where_sql} ORDER BY {sort_field} {sort_dir} LIMIT {limit} OFFSET {offset}"
        );

        // Helper macro-like approach: bind parameters in order for both queries
        // We need to run count and data queries with same bindings.

        // Count query
        let mut count_query = sqlx::query(&count_sql);
        count_query = count_query.bind(req.project_id);
        if let Some(status_id) = req.status_id {
            count_query = count_query.bind(status_id);
        }
        if let Some(ref status_ids) = req.status_ids {
            for sid in status_ids {
                count_query = count_query.bind(*sid);
            }
        }
        if let Some(ref priority) = req.priority {
            count_query = count_query.bind(priority_to_str(priority));
        }
        if let Some(parent_issue_id) = req.parent_issue_id {
            count_query = count_query.bind(parent_issue_id);
        }
        if let Some(ref search) = req.search {
            count_query = count_query.bind(format!("%{search}%"));
        }
        if let Some(ref simple_id) = req.simple_id {
            count_query = count_query.bind(simple_id.clone());
        }

        let count_row = count_query.fetch_one(pool).await?;
        let total_count: i64 = count_row.try_get("cnt")?;

        // Data query
        let mut data_query = sqlx::query(&data_sql);
        data_query = data_query.bind(req.project_id);
        if let Some(status_id) = req.status_id {
            data_query = data_query.bind(status_id);
        }
        if let Some(ref status_ids) = req.status_ids {
            for sid in status_ids {
                data_query = data_query.bind(*sid);
            }
        }
        if let Some(ref priority) = req.priority {
            data_query = data_query.bind(priority_to_str(priority));
        }
        if let Some(parent_issue_id) = req.parent_issue_id {
            data_query = data_query.bind(parent_issue_id);
        }
        if let Some(ref search) = req.search {
            data_query = data_query.bind(format!("%{search}%"));
        }
        if let Some(ref simple_id) = req.simple_id {
            data_query = data_query.bind(simple_id.clone());
        }

        let rows = data_query.fetch_all(pool).await?;
        let issues: Vec<Issue> = rows.iter().map(issue_from_row).collect::<Result<_, _>>()?;

        Ok(ListIssuesResponse {
            issues,
            total_count: total_count as usize,
            limit: limit as usize,
            offset: offset as usize,
        })
    }
}
