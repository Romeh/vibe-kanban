-- Configurable status mappings for Jira integration.
-- Maps VK status names to Jira status category keys (new, indeterminate, done).
CREATE TABLE jira_status_mappings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    vk_status_name TEXT NOT NULL,
    jira_category_key TEXT NOT NULL CHECK (jira_category_key IN ('new', 'indeterminate', 'done')),
    CONSTRAINT jira_status_mappings_uniq UNIQUE (organization_id, vk_status_name)
);
