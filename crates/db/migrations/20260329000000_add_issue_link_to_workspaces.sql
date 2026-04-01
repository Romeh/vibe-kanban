-- Add issue linking columns to local workspaces table for local-only mode.
-- In remote mode these are stored on the remote server; in local mode we need them here.
ALTER TABLE workspaces ADD COLUMN issue_id BLOB;
ALTER TABLE workspaces ADD COLUMN project_id BLOB;
