-- Jira integration: stores per-organization Jira Cloud connections.

CREATE TABLE jira_connections (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    jira_site_url TEXT NOT NULL,
    auth_type TEXT NOT NULL CHECK (auth_type IN ('oauth2', 'api_token')),
    encrypted_credentials BYTEA NOT NULL,
    connected_by_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT jira_connections_organization_id_uniq UNIQUE (organization_id)
);

CREATE INDEX idx_jira_connections_organization_id ON jira_connections(organization_id);
