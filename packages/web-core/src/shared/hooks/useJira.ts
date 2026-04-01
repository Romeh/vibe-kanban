import { useCallback } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type {
  JiraCommentView,
  JiraConnectionInfo,
  JiraConnectRequest,
  JiraImportRequest,
  JiraOAuthAuthorizeResponse,
  JiraProject,
  JiraSearchResult,
  JiraSearchRequest,
  JiraStatusMapping,
  JiraStatusMappingDeleteRequest,
  JiraStatusMappingRequest,
  JiraStatusView,
} from '@/shared/types/jira';
import { makeRequest } from '@/shared/lib/remoteApi';
import { makeLocalApiRequest } from '@/shared/lib/localApiTransport';
import { useAppRuntime } from '@/shared/hooks/useAppRuntime';

// ---------------------------------------------------------------------------
// Runtime-aware fetch helper
// ---------------------------------------------------------------------------

type JiraFetcher = (
  path: string,
  organizationId?: string,
  options?: RequestInit
) => Promise<Response>;

function useJiraFetcher(): JiraFetcher {
  const runtime = useAppRuntime();

  return useCallback(
    (path: string, organizationId?: string, options?: RequestInit) => {
      if (runtime === 'local') {
        const headers = new Headers((options ?? {}).headers ?? {});
        if (!headers.has('Content-Type')) {
          headers.set('Content-Type', 'application/json');
        }
        return makeLocalApiRequest(`/api/jira${path}`, {
          ...(options ?? {}),
          headers,
        });
      }

      const sep = path.includes('?') ? '&' : '?';
      return makeRequest(
        `/v1/jira${path}${sep}organization_id=${organizationId}`,
        options
      );
    },
    [runtime]
  );
}

// ---------------------------------------------------------------------------
// Query keys
// ---------------------------------------------------------------------------

const jiraKeys = {
  connection: (orgId: string) => ['jira', 'connection', orgId] as const,
  projects: (orgId: string) => ['jira', 'projects', orgId] as const,
  search: (orgId: string, query: string) =>
    ['jira', 'search', orgId, query] as const,
};

const LOCAL_KEY = '__local__';

// ---------------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------------

export function useJiraConnection(organizationId: string | undefined) {
  const jiraFetch = useJiraFetcher();
  const runtime = useAppRuntime();
  const effectiveId = runtime === 'local' ? LOCAL_KEY : organizationId;

  return useQuery({
    queryKey: jiraKeys.connection(effectiveId ?? ''),
    queryFn: async (): Promise<JiraConnectionInfo> => {
      const response = await jiraFetch('/connection', organizationId);
      if (!response.ok) {
        throw new Error('Failed to fetch Jira connection status');
      }
      return response.json();
    },
    enabled: !!effectiveId,
  });
}

export function useJiraProjects(organizationId: string | undefined) {
  const jiraFetch = useJiraFetcher();
  const runtime = useAppRuntime();
  const effectiveId = runtime === 'local' ? LOCAL_KEY : organizationId;

  return useQuery({
    queryKey: jiraKeys.projects(effectiveId ?? ''),
    queryFn: async (): Promise<JiraProject[]> => {
      const response = await jiraFetch('/projects', organizationId);
      if (!response.ok) {
        throw new Error('Failed to fetch Jira projects');
      }
      return response.json();
    },
    enabled: !!effectiveId,
  });
}

export function useJiraSearch(
  organizationId: string | undefined,
  searchRequest: JiraSearchRequest | null
) {
  const jiraFetch = useJiraFetcher();
  const runtime = useAppRuntime();
  const effectiveId = runtime === 'local' ? LOCAL_KEY : organizationId;

  return useQuery({
    queryKey: [
      'jira',
      'search',
      effectiveId ?? '',
      searchRequest?.query ?? '',
      searchRequest?.project_key ?? null,
      searchRequest?.max_results ?? null,
    ],
    queryFn: async (): Promise<JiraSearchResult> => {
      const response = await jiraFetch('/search', organizationId, {
        method: 'POST',
        body: JSON.stringify(searchRequest),
      });
      if (!response.ok) {
        throw new Error('Failed to search Jira issues');
      }
      return response.json();
    },
    enabled: !!effectiveId && !!searchRequest?.query,
  });
}

export function useJiraConnect(organizationId: string) {
  const jiraFetch = useJiraFetcher();
  const runtime = useAppRuntime();
  const effectiveId = runtime === 'local' ? LOCAL_KEY : organizationId;
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (
      payload: JiraConnectRequest
    ): Promise<JiraConnectionInfo> => {
      const response = await jiraFetch('/connect', organizationId, {
        method: 'POST',
        body: JSON.stringify(payload),
      });
      if (!response.ok) {
        const err = await response
          .json()
          .catch(() => ({ error: 'Unknown error' }));
        throw new Error(err.error || 'Failed to connect to Jira');
      }
      return response.json();
    },
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: jiraKeys.connection(effectiveId),
      });
    },
  });
}

export function useJiraDisconnect(organizationId: string) {
  const jiraFetch = useJiraFetcher();
  const runtime = useAppRuntime();
  const effectiveId = runtime === 'local' ? LOCAL_KEY : organizationId;
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (): Promise<void> => {
      const response = await jiraFetch('/connect', organizationId, {
        method: 'DELETE',
      });
      if (!response.ok) {
        throw new Error('Failed to disconnect Jira');
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: jiraKeys.connection(effectiveId),
      });
      queryClient.invalidateQueries({
        queryKey: jiraKeys.projects(effectiveId),
      });
    },
  });
}

export function useJiraImport(organizationId: string) {
  const jiraFetch = useJiraFetcher();

  return useMutation({
    mutationFn: async (
      payload: JiraImportRequest
    ): Promise<{ data: unknown[]; error: string | null }> => {
      const response = await jiraFetch('/import', organizationId, {
        method: 'POST',
        body: JSON.stringify(payload),
      });
      if (!response.ok) {
        const err = await response
          .json()
          .catch(() => ({ error: 'Import failed' }));
        throw new Error(err.error || 'Failed to import Jira issues');
      }
      return response.json();
    },
  });
}

export function useJiraOAuthUrl(organizationId: string | undefined) {
  const jiraFetch = useJiraFetcher();

  return useMutation({
    mutationFn: async (): Promise<JiraOAuthAuthorizeResponse> => {
      const response = await jiraFetch('/oauth/authorize', organizationId);
      if (!response.ok) {
        const err = await response
          .json()
          .catch(() => ({ error: 'Failed to get OAuth URL' }));
        throw new Error(err.error || 'Failed to get Jira OAuth URL');
      }
      return response.json();
    },
  });
}

export function useJiraSyncStatus() {
  const jiraFetch = useJiraFetcher();

  return useMutation({
    mutationFn: async (issueId: string): Promise<void> => {
      const response = await jiraFetch(`/sync-status/${issueId}`, undefined, {
        method: 'POST',
      });
      if (!response.ok) {
        const err = await response
          .json()
          .catch(() => ({ error: 'Sync failed' }));
        throw new Error(err.error || 'Failed to sync status to Jira');
      }
    },
  });
}

// ---------------------------------------------------------------------------
// Jira comments
// ---------------------------------------------------------------------------

export function useJiraComments(issueId: string | undefined) {
  const jiraFetch = useJiraFetcher();

  return useQuery({
    queryKey: ['jira', 'comments', issueId ?? ''],
    queryFn: async (): Promise<JiraCommentView[]> => {
      const response = await jiraFetch(`/comments/${issueId}`);
      if (!response.ok) {
        throw new Error('Failed to fetch Jira comments');
      }
      return response.json();
    },
    enabled: !!issueId,
  });
}

export function useJiraPostSummary() {
  const jiraFetch = useJiraFetcher();

  return useMutation({
    mutationFn: async (issueId: string): Promise<void> => {
      const response = await jiraFetch(`/post-summary/${issueId}`, undefined, {
        method: 'POST',
      });
      if (!response.ok) {
        const err = await response
          .json()
          .catch(() => ({ error: 'Failed to post summary' }));
        throw new Error(err.error || 'Failed to post summary to Jira');
      }
    },
  });
}

// ---------------------------------------------------------------------------
// Status mappings
// ---------------------------------------------------------------------------

const statusMappingsKey = (orgId: string) =>
  ['jira', 'status-mappings', orgId] as const;

export function useJiraStatuses(organizationId: string | undefined) {
  const jiraFetch = useJiraFetcher();
  const runtime = useAppRuntime();
  const effectiveId = runtime === 'local' ? LOCAL_KEY : organizationId;

  return useQuery({
    queryKey: ['jira', 'statuses', effectiveId ?? ''],
    queryFn: async (): Promise<JiraStatusView[]> => {
      const response = await jiraFetch('/statuses', organizationId);
      if (!response.ok) {
        throw new Error('Failed to fetch Jira statuses');
      }
      return response.json();
    },
    enabled: !!effectiveId,
    staleTime: 5 * 60 * 1000, // Cache for 5 minutes
  });
}

export function useJiraStatusMappings(organizationId: string | undefined) {
  const jiraFetch = useJiraFetcher();
  const runtime = useAppRuntime();
  const effectiveId = runtime === 'local' ? LOCAL_KEY : organizationId;

  return useQuery({
    queryKey: statusMappingsKey(effectiveId ?? ''),
    queryFn: async (): Promise<JiraStatusMapping[]> => {
      const response = await jiraFetch('/status-mappings', organizationId);
      if (!response.ok) {
        throw new Error('Failed to fetch status mappings');
      }
      return response.json();
    },
    enabled: !!effectiveId,
  });
}

export function useJiraUpsertStatusMapping(organizationId: string) {
  const jiraFetch = useJiraFetcher();
  const runtime = useAppRuntime();
  const effectiveId = runtime === 'local' ? LOCAL_KEY : organizationId;
  const queryClient = useQueryClient();
  const key = statusMappingsKey(effectiveId);

  return useMutation({
    mutationFn: async (payload: JiraStatusMappingRequest): Promise<void> => {
      const response = await jiraFetch(
        '/status-mappings/upsert',
        organizationId,
        { method: 'POST', body: JSON.stringify(payload) }
      );
      if (!response.ok) {
        const err = await response
          .json()
          .catch(() => ({ error: 'Unknown error' }));
        throw new Error(err.error || 'Failed to save status mapping');
      }
    },
    onMutate: async (payload) => {
      await queryClient.cancelQueries({ queryKey: key });
      const previous = queryClient.getQueryData<JiraStatusMapping[]>(key);
      queryClient.setQueryData<JiraStatusMapping[]>(key, (old = []) => {
        const filtered = old.filter(
          (m) => m.vk_status_name !== payload.vk_status_name
        );
        return [...filtered, payload];
      });
      return { previous };
    },
    onError: (_err, _payload, context) => {
      if (context?.previous) {
        queryClient.setQueryData(key, context.previous);
      }
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: key });
    },
  });
}

export function useJiraDeleteStatusMapping(organizationId: string) {
  const jiraFetch = useJiraFetcher();
  const runtime = useAppRuntime();
  const effectiveId = runtime === 'local' ? LOCAL_KEY : organizationId;
  const queryClient = useQueryClient();
  const key = statusMappingsKey(effectiveId);

  return useMutation({
    mutationFn: async (
      payload: JiraStatusMappingDeleteRequest
    ): Promise<void> => {
      const response = await jiraFetch(
        '/status-mappings/delete',
        organizationId,
        { method: 'POST', body: JSON.stringify(payload) }
      );
      if (!response.ok) {
        throw new Error('Failed to delete status mapping');
      }
    },
    onMutate: async (payload) => {
      await queryClient.cancelQueries({ queryKey: key });
      const previous = queryClient.getQueryData<JiraStatusMapping[]>(key);
      queryClient.setQueryData<JiraStatusMapping[]>(key, (old = []) =>
        old.filter((m) => m.vk_status_name !== payload.vk_status_name)
      );
      return { previous };
    },
    onError: (_err, _payload, context) => {
      if (context?.previous) {
        queryClient.setQueryData(key, context.previous);
      }
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: key });
    },
  });
}
