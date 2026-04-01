PRAGMA foreign_keys = ON;

-- Extend tasks with Jira metadata and priority support
ALTER TABLE tasks ADD COLUMN extension_metadata TEXT NOT NULL DEFAULT '{}';
ALTER TABLE tasks ADD COLUMN priority TEXT;

-- Jira connection (single-user, max one row)
CREATE TABLE jira_connections (
    id                    BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    jira_site_url         TEXT NOT NULL,
    auth_type             TEXT NOT NULL CHECK (auth_type IN ('api_token', 'oauth2')),
    encrypted_credentials BLOB NOT NULL,
    created_at            TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at            TEXT NOT NULL DEFAULT (datetime('now', 'subsec'))
);

-- Status mappings (single-user)
CREATE TABLE jira_status_mappings (
    id                BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    vk_status_name    TEXT NOT NULL UNIQUE,
    jira_category_key TEXT NOT NULL CHECK (jira_category_key IN ('new', 'indeterminate', 'done'))
);
