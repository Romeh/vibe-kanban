PRAGMA foreign_keys = ON;

-- Add organization_id and identifier to projects for local kanban mode
ALTER TABLE projects ADD COLUMN organization_id BLOB;
ALTER TABLE projects ADD COLUMN identifier TEXT NOT NULL DEFAULT '';
ALTER TABLE projects ADD COLUMN color TEXT NOT NULL DEFAULT '#6b7280';
ALTER TABLE projects ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;

-- Project statuses (kanban columns)
CREATE TABLE project_statuses (
    id         BLOB PRIMARY KEY,
    project_id BLOB NOT NULL,
    name       TEXT NOT NULL,
    color      TEXT NOT NULL DEFAULT '#6b7280',
    sort_order INTEGER NOT NULL DEFAULT 0,
    hidden     INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX idx_project_statuses_project_id ON project_statuses(project_id);

-- Issues (full-featured kanban items, matches api_types::Issue)
CREATE TABLE issues (
    id                      BLOB PRIMARY KEY,
    project_id              BLOB NOT NULL,
    issue_number            INTEGER NOT NULL,
    simple_id               TEXT NOT NULL,
    status_id               BLOB NOT NULL,
    title                   TEXT NOT NULL,
    description             TEXT,
    priority                TEXT CHECK(priority IN ('urgent', 'high', 'medium', 'low')),
    start_date              TEXT,
    target_date             TEXT,
    completed_at            TEXT,
    sort_order              REAL NOT NULL DEFAULT 0,
    parent_issue_id         BLOB,
    parent_issue_sort_order REAL,
    extension_metadata      TEXT NOT NULL DEFAULT '{}',
    creator_user_id         BLOB,
    created_at              TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at              TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY (status_id) REFERENCES project_statuses(id)
);

CREATE INDEX idx_issues_project_id ON issues(project_id);
CREATE INDEX idx_issues_status_id ON issues(status_id);

-- Auto-increment sequence for issue_number per project
CREATE TABLE issue_sequences (
    project_id  BLOB PRIMARY KEY,
    next_number INTEGER NOT NULL DEFAULT 1,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

-- Project-scoped tags (kanban labels, matches api_types::Tag)
CREATE TABLE project_tags (
    id         BLOB PRIMARY KEY,
    project_id BLOB NOT NULL,
    name       TEXT NOT NULL,
    color      TEXT NOT NULL DEFAULT '#6b7280',
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX idx_project_tags_project_id ON project_tags(project_id);

-- Issue-tag junction (matches api_types::IssueTag)
CREATE TABLE issue_tags (
    id       BLOB PRIMARY KEY,
    issue_id BLOB NOT NULL,
    tag_id   BLOB NOT NULL,
    FOREIGN KEY (issue_id) REFERENCES issues(id) ON DELETE CASCADE,
    FOREIGN KEY (tag_id)   REFERENCES project_tags(id) ON DELETE CASCADE,
    UNIQUE(issue_id, tag_id)
);

CREATE INDEX idx_issue_tags_issue_id ON issue_tags(issue_id);

-- Issue assignees (matches api_types::IssueAssignee)
CREATE TABLE issue_assignees (
    id          BLOB PRIMARY KEY,
    issue_id    BLOB NOT NULL,
    user_id     BLOB NOT NULL,
    assigned_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (issue_id) REFERENCES issues(id) ON DELETE CASCADE,
    UNIQUE(issue_id, user_id)
);

CREATE INDEX idx_issue_assignees_issue_id ON issue_assignees(issue_id);

-- Issue followers (matches api_types::IssueFollower)
CREATE TABLE issue_followers (
    id       BLOB PRIMARY KEY,
    issue_id BLOB NOT NULL,
    user_id  BLOB NOT NULL,
    FOREIGN KEY (issue_id) REFERENCES issues(id) ON DELETE CASCADE,
    UNIQUE(issue_id, user_id)
);

-- Issue relationships (matches api_types::IssueRelationship)
CREATE TABLE issue_relationships (
    id                BLOB PRIMARY KEY,
    issue_id          BLOB NOT NULL,
    related_issue_id  BLOB NOT NULL,
    relationship_type TEXT NOT NULL CHECK(relationship_type IN ('blocking', 'related', 'has_duplicate')),
    created_at        TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (issue_id)         REFERENCES issues(id) ON DELETE CASCADE,
    FOREIGN KEY (related_issue_id) REFERENCES issues(id) ON DELETE CASCADE
);

CREATE INDEX idx_issue_relationships_issue_id ON issue_relationships(issue_id);

-- Issue comments (matches api_types::IssueComment)
CREATE TABLE issue_comments (
    id         BLOB PRIMARY KEY,
    issue_id   BLOB NOT NULL,
    author_id  BLOB,
    parent_id  BLOB,
    message    TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (issue_id)  REFERENCES issues(id) ON DELETE CASCADE,
    FOREIGN KEY (parent_id) REFERENCES issue_comments(id) ON DELETE SET NULL
);

CREATE INDEX idx_issue_comments_issue_id ON issue_comments(issue_id);

-- Issue comment reactions (matches api_types::IssueCommentReaction)
CREATE TABLE issue_comment_reactions (
    id         BLOB PRIMARY KEY,
    comment_id BLOB NOT NULL,
    user_id    BLOB NOT NULL,
    emoji      TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (comment_id) REFERENCES issue_comments(id) ON DELETE CASCADE,
    UNIQUE(comment_id, user_id, emoji)
);

-- Pull request issues junction (matches api_types::PullRequestIssue)
CREATE TABLE pull_request_issues (
    id              BLOB PRIMARY KEY,
    pull_request_id BLOB NOT NULL,
    issue_id        BLOB NOT NULL,
    FOREIGN KEY (issue_id) REFERENCES issues(id) ON DELETE CASCADE,
    UNIQUE(pull_request_id, issue_id)
);
