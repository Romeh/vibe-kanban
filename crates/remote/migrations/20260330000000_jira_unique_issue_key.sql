-- Prevent importing the same Jira issue key twice into the same project.
-- Uses a partial expression index on the JSONB field to enforce uniqueness at the
-- database level, closing the application-level TOCTOU race window.
CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS idx_issues_jira_key_unique
    ON issues (project_id, ((extension_metadata -> 'jira' ->> 'issue_key')))
    WHERE extension_metadata -> 'jira' ->> 'issue_key' IS NOT NULL;
