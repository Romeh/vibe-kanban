import { useState, useCallback, useMemo, useEffect } from 'react';
import {
  SpinnerIcon,
  PlugIcon,
  TrashIcon,
  CheckCircleIcon,
  WarningCircleIcon,
  GlobeIcon,
} from '@phosphor-icons/react';
import {
  useJiraConnection,
  useJiraConnect,
  useJiraDisconnect,
  useJiraOAuthUrl,
  useJiraStatuses,
  useJiraStatusMappings,
  useJiraUpsertStatusMapping,
  useJiraDeleteStatusMapping,
} from '@/shared/hooks/useJira';
import { PlusIcon } from '@phosphor-icons/react';
import { useAuth } from '@/shared/hooks/auth/useAuth';
import { useUserOrganizations } from '@/shared/hooks/useUserOrganizations';
import { useOrganizationSelection } from '@/shared/hooks/useOrganizationSelection';
import { useAppRuntime } from '@/shared/hooks/useAppRuntime';
import { PrimaryButton } from '@vibe/ui/components/PrimaryButton';
import { SettingsCard, SettingsField } from './SettingsComponents';

export function JiraSettingsSection() {
  const { isSignedIn } = useAuth();
  const runtime = useAppRuntime();
  const isLocal = runtime === 'local';
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  // Organization selection (remote only)
  const { data: orgsResponse } = useUserOrganizations();
  const orgList = useMemo(
    () => (isLocal ? [] : (orgsResponse?.organizations ?? [])),
    [orgsResponse, isLocal]
  );
  const { selectedOrgId, handleOrgSelect } = useOrganizationSelection({
    organizations: isLocal ? undefined : orgsResponse,
    onSelectionChange: () => {
      setSuccess(null);
      setError(null);
    },
  });

  // In local mode, use a sentinel value so hooks are always enabled
  const effectiveOrgId = isLocal ? '__local__' : selectedOrgId;

  // Jira connection state
  const { data: connection, isLoading: connectionLoading } =
    useJiraConnection(effectiveOrgId);

  const connectMutation = useJiraConnect(effectiveOrgId ?? '');
  const disconnectMutation = useJiraDisconnect(effectiveOrgId ?? '');
  const oauthUrlMutation = useJiraOAuthUrl(effectiveOrgId);

  // Status mappings
  const { data: statusMappings } = useJiraStatusMappings(effectiveOrgId);
  const { data: jiraStatuses } = useJiraStatuses(effectiveOrgId);
  const upsertMapping = useJiraUpsertStatusMapping(effectiveOrgId ?? '');
  const deleteMapping = useJiraDeleteStatusMapping(effectiveOrgId ?? '');
  const [newMappingVkStatus, setNewMappingVkStatus] = useState('');
  const [newMappingCategory, setNewMappingCategory] = useState('');
  const [mappingError, setMappingError] = useState<string | null>(null);

  // API token form state
  const [siteUrl, setSiteUrl] = useState('');
  const [email, setEmail] = useState('');
  const [apiToken, setApiToken] = useState('');
  const [connecting, setConnecting] = useState(false);
  const [disconnecting, setDisconnecting] = useState(false);
  const [authMethod, setAuthMethod] = useState<'oauth' | 'api_token'>(
    isLocal ? 'api_token' : 'oauth'
  );

  // Handle OAuth callback: check URL params for ?code=...&state=...
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const code = params.get('code');
    const state = params.get('state');
    if (!code || !selectedOrgId) return;

    // Verify state starts with our org ID (CSRF protection).
    // The server appends an HMAC signature: "{org_id}:{sig}".
    const stateOrgId = state?.split(':')[0];
    if (!state || stateOrgId !== selectedOrgId) {
      if (code)
        setError('OAuth verification failed. Please try connecting again.');
      return;
    }

    // Clear URL params to avoid re-processing
    const url = new URL(window.location.href);
    url.searchParams.delete('code');
    url.searchParams.delete('state');
    window.history.replaceState({}, '', url.toString());

    // Complete the OAuth flow
    const redirectUri = `${window.location.origin}/jira/oauth/callback`;
    setConnecting(true);
    setError(null);
    connectMutation
      .mutateAsync({
        site_url: 'auto',
        auth: { type: 'oauth2', code, redirect_uri: redirectUri },
      })
      .then(() => {
        setSuccess('Successfully connected to Jira via OAuth!');
        setTimeout(() => setSuccess(null), 5000);
      })
      .catch((err) => {
        setError(
          err instanceof Error ? err.message : 'OAuth connection failed'
        );
      })
      .finally(() => setConnecting(false));
  }, [selectedOrgId]); // eslint-disable-line react-hooks/exhaustive-deps

  const handleOAuthConnect = useCallback(async () => {
    setError(null);
    try {
      const { authorize_url } = await oauthUrlMutation.mutateAsync();
      // Redirect to Atlassian authorization page
      window.location.href = authorize_url;
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : 'Failed to start OAuth flow — Jira OAuth may not be configured on this server'
      );
    }
  }, [oauthUrlMutation]);

  const handleConnect = useCallback(async () => {
    if (!siteUrl || !email || !apiToken) {
      setError('Please fill in all fields');
      return;
    }

    setConnecting(true);
    setError(null);
    try {
      await connectMutation.mutateAsync({
        site_url: siteUrl,
        auth: {
          type: 'api_token',
          email,
          token: apiToken,
        },
      });
      setSuccess('Successfully connected to Jira!');
      setSiteUrl('');
      setEmail('');
      setApiToken('');
      setTimeout(() => setSuccess(null), 5000);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to connect to Jira'
      );
    } finally {
      setConnecting(false);
    }
  }, [siteUrl, email, apiToken, connectMutation]);

  const handleDisconnect = useCallback(async () => {
    setDisconnecting(true);
    setError(null);
    try {
      await disconnectMutation.mutateAsync();
      setSuccess('Jira disconnected');
      setTimeout(() => setSuccess(null), 3000);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to disconnect Jira'
      );
    } finally {
      setDisconnecting(false);
    }
  }, [disconnectMutation]);

  if (!isLocal && !isSignedIn) {
    return (
      <SettingsCard
        title="Jira Integration"
        description="Sign in to connect your Jira account."
      >
        <p className="text-low text-sm">
          Sign in to your Vibe Kanban account to configure Jira integration.
        </p>
      </SettingsCard>
    );
  }

  const isConnected = connection?.connected ?? false;

  return (
    <>
      {error && (
        <div className="flex items-center gap-2 bg-error/10 border border-error/30 rounded p-3 text-error text-sm">
          <WarningCircleIcon size={16} weight="fill" />
          {error}
        </div>
      )}

      {success && (
        <div className="flex items-center gap-2 bg-success/10 border border-success/30 rounded p-3 text-success text-sm">
          <CheckCircleIcon size={16} weight="fill" />
          {success}
        </div>
      )}

      {/* Organization selector (remote mode, when user has multiple orgs) */}
      {!isLocal && orgList.length > 1 && (
        <SettingsCard title="Organization" description="">
          <SettingsField label="Select organization">
            <select
              className="w-full px-base py-1.5 bg-secondary rounded border text-sm text-normal"
              value={selectedOrgId ?? ''}
              onChange={(e) => handleOrgSelect(e.target.value)}
            >
              {orgList.map((org) => (
                <option key={org.id} value={org.id}>
                  {org.name}
                </option>
              ))}
            </select>
          </SettingsField>
        </SettingsCard>
      )}

      {/* Connection status */}
      <SettingsCard
        title="Jira Connection"
        description="Connect your Atlassian Jira Cloud instance to import issues into Vibe Kanban."
      >
        {connectionLoading ? (
          <div className="flex items-center gap-2 text-low text-sm">
            <SpinnerIcon size={16} className="animate-spin" />
            Loading connection status...
          </div>
        ) : isConnected ? (
          <div className="space-y-3">
            <div className="flex items-center gap-2 text-success text-sm">
              <PlugIcon size={16} weight="fill" />
              Connected to{' '}
              <span className="font-medium">{connection?.site_url}</span>
            </div>
            <div className="text-low text-xs">
              Auth type: {connection?.auth_type} &middot; Connected{' '}
              {connection?.connected_at
                ? new Date(connection.connected_at).toLocaleDateString()
                : ''}
            </div>
            <button
              onClick={handleDisconnect}
              disabled={disconnecting}
              className="flex items-center gap-1.5 px-3 py-1.5 text-sm text-error hover:bg-error/10 rounded border border-error/30 transition-colors"
            >
              <TrashIcon size={14} />
              {disconnecting ? 'Disconnecting...' : 'Disconnect'}
            </button>
          </div>
        ) : (
          <div className="space-y-4">
            {/* Auth method tabs (OAuth hidden in local mode) */}
            {!isLocal && (
              <div className="flex gap-1 border-b">
                <button
                  type="button"
                  onClick={() => setAuthMethod('oauth')}
                  className={`px-3 py-1.5 text-sm transition-colors border-b-2 -mb-px ${
                    authMethod === 'oauth'
                      ? 'border-brand text-brand'
                      : 'border-transparent text-low hover:text-normal'
                  }`}
                >
                  Connect with Atlassian
                </button>
                <button
                  type="button"
                  onClick={() => setAuthMethod('api_token')}
                  className={`px-3 py-1.5 text-sm transition-colors border-b-2 -mb-px ${
                    authMethod === 'api_token'
                      ? 'border-brand text-brand'
                      : 'border-transparent text-low hover:text-normal'
                  }`}
                >
                  API Token
                </button>
              </div>
            )}

            {authMethod === 'oauth' ? (
              <div className="space-y-3">
                <p className="text-low text-sm">
                  Connect your Atlassian account using OAuth. This is the
                  recommended method for Jira Cloud.
                </p>
                <PrimaryButton
                  onClick={handleOAuthConnect}
                  disabled={connecting || oauthUrlMutation.isPending}
                >
                  {connecting || oauthUrlMutation.isPending ? (
                    <>
                      <SpinnerIcon size={14} className="animate-spin" />
                      Connecting...
                    </>
                  ) : (
                    <>
                      <GlobeIcon size={14} />
                      Connect with Atlassian
                    </>
                  )}
                </PrimaryButton>
              </div>
            ) : (
              <div className="space-y-4">
                <p className="text-low text-sm">
                  Enter your Atlassian site URL and an API token to connect. You
                  can create an API token at{' '}
                  <a
                    href="https://id.atlassian.com/manage-profile/security/api-tokens"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-brand underline"
                  >
                    id.atlassian.com
                  </a>
                  .
                </p>

                <SettingsField label="Site URL">
                  <input
                    type="url"
                    placeholder="https://yourcompany.atlassian.net"
                    value={siteUrl}
                    onChange={(e) => setSiteUrl(e.target.value)}
                    className="w-full px-base py-1.5 bg-secondary rounded border text-sm text-normal placeholder:text-low focus:outline-none focus:ring-1 focus:ring-brand"
                  />
                </SettingsField>

                <SettingsField label="Email">
                  <input
                    type="email"
                    placeholder="you@company.com"
                    value={email}
                    onChange={(e) => setEmail(e.target.value)}
                    className="w-full px-base py-1.5 bg-secondary rounded border text-sm text-normal placeholder:text-low focus:outline-none focus:ring-1 focus:ring-brand"
                  />
                </SettingsField>

                <SettingsField label="API Token">
                  <input
                    type="password"
                    placeholder="Your Jira API token"
                    value={apiToken}
                    onChange={(e) => setApiToken(e.target.value)}
                    className="w-full px-base py-1.5 bg-secondary rounded border text-sm text-normal placeholder:text-low focus:outline-none focus:ring-1 focus:ring-brand"
                  />
                </SettingsField>

                <PrimaryButton
                  onClick={handleConnect}
                  disabled={connecting || !siteUrl || !email || !apiToken}
                >
                  {connecting ? (
                    <>
                      <SpinnerIcon size={14} className="animate-spin" />
                      Connecting...
                    </>
                  ) : (
                    <>
                      <PlugIcon size={14} />
                      Connect to Jira
                    </>
                  )}
                </PrimaryButton>
              </div>
            )}
          </div>
        )}
      </SettingsCard>

      {/* Status Mapping Config (visible only when connected) */}
      {isConnected && (
        <SettingsCard
          title="Status Mappings"
          description="Configure how Vibe Kanban statuses map to Jira status categories. Custom mappings override the defaults."
        >
          <div className="space-y-3">
            {/* Existing mappings */}
            {statusMappings && statusMappings.length > 0 && (
              <div className="space-y-1.5">
                {statusMappings.map((m) => (
                  <div
                    key={m.vk_status_name}
                    className="flex items-center gap-2 text-sm"
                  >
                    <span className="flex-1 text-normal">
                      {m.vk_status_name}
                    </span>
                    <span className="text-low">→</span>
                    <span className="text-normal w-28 text-right">
                      {{
                        new: 'To Do',
                        indeterminate: 'In Progress',
                        done: 'Done',
                      }[m.jira_category_key] ?? m.jira_category_key}
                    </span>
                    <button
                      type="button"
                      onClick={() =>
                        deleteMapping.mutate({
                          vk_status_name: m.vk_status_name,
                        })
                      }
                      className="p-0.5 text-low hover:text-error transition-colors"
                    >
                      <TrashIcon size={12} />
                    </button>
                  </div>
                ))}
              </div>
            )}

            {/* Default mappings info */}
            <div className="text-xs text-low">
              Defaults: Done/Cancelled → Done, In Progress/In Review → In
              Progress, To Do/Backlog → To Do
            </div>

            {mappingError && (
              <div className="text-xs text-error">{mappingError}</div>
            )}

            {/* Add new mapping */}
            <div className="flex items-center gap-2">
              <input
                type="text"
                placeholder="VK status name"
                value={newMappingVkStatus}
                onChange={(e) => setNewMappingVkStatus(e.target.value)}
                className="flex-1 px-2 py-1 bg-secondary rounded border text-sm text-normal placeholder:text-low focus:outline-none focus:ring-1 focus:ring-brand"
              />
              <select
                value={newMappingCategory}
                onChange={(e) => setNewMappingCategory(e.target.value)}
                className="w-44 px-2 py-1 bg-secondary rounded border text-sm text-normal"
              >
                <option value="">Select Jira status...</option>
                {jiraStatuses && jiraStatuses.length > 0 ? (
                  <>
                    {['new', 'indeterminate', 'done'].map((catKey) => {
                      const categoryStatuses = jiraStatuses.filter(
                        (s) => s.category_key === catKey
                      );
                      if (categoryStatuses.length === 0) return null;
                      return (
                        <optgroup
                          key={catKey}
                          label={categoryStatuses[0]?.category_name ?? catKey}
                        >
                          {categoryStatuses.map((s) => (
                            <option key={s.name} value={s.category_key}>
                              {s.name}
                            </option>
                          ))}
                        </optgroup>
                      );
                    })}
                  </>
                ) : (
                  <>
                    <option value="new">To Do</option>
                    <option value="indeterminate">In Progress</option>
                    <option value="done">Done</option>
                  </>
                )}
              </select>
              <button
                type="button"
                onClick={() => {
                  if (!newMappingVkStatus.trim() || !newMappingCategory) return;
                  setMappingError(null);
                  upsertMapping.mutate(
                    {
                      vk_status_name: newMappingVkStatus.trim(),
                      jira_category_key: newMappingCategory,
                    },
                    {
                      onSuccess: () => {
                        setNewMappingVkStatus('');
                        setNewMappingCategory('');
                      },
                      onError: (err) => {
                        setMappingError(
                          err instanceof Error ? err.message : 'Failed to save'
                        );
                      },
                    }
                  );
                }}
                disabled={
                  !newMappingVkStatus.trim() ||
                  !newMappingCategory ||
                  upsertMapping.isPending
                }
                className="p-1 text-brand hover:bg-brand/10 rounded transition-colors disabled:opacity-50"
              >
                <PlusIcon size={16} />
              </button>
            </div>
          </div>
        </SettingsCard>
      )}
    </>
  );
}
