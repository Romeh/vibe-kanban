import { useState, useCallback, useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import { create, useModal } from '@ebay/nice-modal-react';
import { defineModal } from '@/shared/lib/modals';
import {
  MagnifyingGlassIcon,
  SpinnerIcon,
  XIcon,
  ArrowSquareOutIcon,
  CheckIcon,
} from '@phosphor-icons/react';
import { useJiraSearch, useJiraImport } from '@/shared/hooks/useJira';
import type { JiraIssue } from '@/shared/types/jira';

export interface JiraImportDialogProps {
  organizationId: string;
  projectId: string;
  statusId: string;
  siteUrl: string;
}

const JiraImportDialogImpl = create<JiraImportDialogProps>(
  ({ organizationId, projectId, statusId, siteUrl }) => {
    const modal = useModal();
    const [searchQuery, setSearchQuery] = useState('');
    const [debouncedQuery, setDebouncedQuery] = useState('');
    const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());
    const [importing, setImporting] = useState(false);
    const [importResult, setImportResult] = useState<string | null>(null);
    const [error, setError] = useState<string | null>(null);

    const importMutation = useJiraImport(organizationId);
    const closeTimerRef = useRef<ReturnType<typeof setTimeout>>();

    useEffect(() => () => clearTimeout(closeTimerRef.current), []);

    // Debounce search
    useEffect(() => {
      const timer = setTimeout(() => setDebouncedQuery(searchQuery), 400);
      return () => clearTimeout(timer);
    }, [searchQuery]);

    const {
      data: searchResults,
      isLoading: searching,
      error: searchError,
    } = useJiraSearch(
      organizationId,
      debouncedQuery ? { query: debouncedQuery, max_results: 20 } : null
    );

    const handleClose = useCallback(() => {
      modal.hide();
      modal.resolve();
      modal.remove();
    }, [modal]);

    const toggleSelect = useCallback((key: string) => {
      setSelectedKeys((prev) => {
        const next = new Set(prev);
        if (next.has(key)) {
          next.delete(key);
        } else {
          next.add(key);
        }
        return next;
      });
    }, []);

    const handleImport = useCallback(async () => {
      if (selectedKeys.size === 0) return;

      setImporting(true);
      setError(null);
      try {
        const results = await importMutation.mutateAsync({
          project_id: projectId,
          status_id: statusId,
          issue_keys: Array.from(selectedKeys),
        });
        const imported = results?.data ?? [];
        const count = Array.isArray(imported)
          ? imported.length
          : selectedKeys.size;
        setImportResult(
          `Successfully imported ${count} issue${count !== 1 ? 's' : ''}!`
        );
        setSelectedKeys(new Set());
        closeTimerRef.current = setTimeout(() => handleClose(), 2000);
      } catch (err) {
        setError(
          err instanceof Error ? err.message : 'Failed to import issues'
        );
      } finally {
        setImporting(false);
      }
    }, [selectedKeys, importMutation, projectId, statusId, handleClose]);

    // ESC to close
    useEffect(() => {
      const handleKeyDown = (e: KeyboardEvent) => {
        if (e.key === 'Escape') handleClose();
      };
      window.addEventListener('keydown', handleKeyDown);
      return () => window.removeEventListener('keydown', handleKeyDown);
    }, [handleClose]);

    return createPortal(
      <>
        {/* Backdrop */}
        <div
          className="fixed inset-0 z-[9998] bg-black/50"
          onClick={handleClose}
        />

        {/* Dialog */}
        <div className="fixed inset-0 z-[9999] flex items-center justify-center p-4">
          <div
            className="bg-primary border rounded-lg shadow-2xl w-full max-w-2xl max-h-[80vh] flex flex-col"
            onClick={(e) => e.stopPropagation()}
          >
            {/* Header */}
            <div className="flex items-center justify-between p-4 border-b">
              <h2 className="text-lg font-semibold text-high">
                Import from Jira
              </h2>
              <button
                onClick={handleClose}
                className="p-1 text-low hover:text-normal rounded"
              >
                <XIcon size={18} />
              </button>
            </div>

            {/* Search */}
            <div className="p-4 border-b">
              <div className="relative">
                <MagnifyingGlassIcon
                  size={16}
                  className="absolute left-3 top-1/2 -translate-y-1/2 text-low"
                />
                <input
                  type="text"
                  placeholder="Search Jira issues by text or enter JQL..."
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  autoFocus
                  className="w-full pl-9 pr-3 py-2 bg-secondary rounded border text-sm text-normal placeholder:text-low focus:outline-none focus:ring-1 focus:ring-brand"
                />
              </div>
            </div>

            {/* Results */}
            <div className="flex-1 overflow-y-auto p-2 min-h-[200px]">
              {error && (
                <div className="p-3 mb-2 text-sm text-error bg-error/10 rounded">
                  {error}
                </div>
              )}

              {importResult && (
                <div className="p-3 mb-2 text-sm text-success bg-success/10 rounded flex items-center gap-2">
                  <CheckIcon size={16} weight="bold" />
                  {importResult}
                </div>
              )}

              {!searching && searchError && (
                <div className="p-3 mb-2 text-sm text-error bg-error/10 rounded">
                  {searchError instanceof Error
                    ? searchError.message
                    : 'Search failed — check your Jira connection'}
                </div>
              )}

              {searching && (
                <div className="flex items-center justify-center gap-2 py-8 text-low text-sm">
                  <SpinnerIcon size={16} className="animate-spin" />
                  Searching Jira...
                </div>
              )}

              {!searching && !debouncedQuery && (
                <div className="flex items-center justify-center py-8 text-low text-sm">
                  Type to search for Jira issues
                </div>
              )}

              {!searching &&
                debouncedQuery &&
                searchResults?.issues.length === 0 && (
                  <div className="flex items-center justify-center py-8 text-low text-sm">
                    No issues found
                  </div>
                )}

              {searchResults?.issues.map((issue: JiraIssue) => (
                <JiraIssueRow
                  key={issue.key}
                  issue={issue}
                  siteUrl={siteUrl}
                  selected={selectedKeys.has(issue.key)}
                  onToggle={() => toggleSelect(issue.key)}
                />
              ))}
            </div>

            {/* Footer */}
            <div className="flex items-center justify-between p-4 border-t">
              <span className="text-sm text-low">
                {selectedKeys.size > 0
                  ? `${selectedKeys.size} issue${selectedKeys.size !== 1 ? 's' : ''} selected`
                  : 'Select issues to import'}
              </span>
              <div className="flex gap-2">
                <button
                  onClick={handleClose}
                  className="px-3 py-1.5 text-sm text-normal hover:bg-secondary rounded border"
                >
                  Cancel
                </button>
                <button
                  onClick={handleImport}
                  disabled={selectedKeys.size === 0 || importing}
                  className="px-3 py-1.5 text-sm font-medium text-white bg-brand hover:bg-brand/90 rounded disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-1.5"
                >
                  {importing ? (
                    <>
                      <SpinnerIcon size={14} className="animate-spin" />
                      Importing...
                    </>
                  ) : (
                    `Import ${selectedKeys.size > 0 ? selectedKeys.size : ''} Issue${selectedKeys.size !== 1 ? 's' : ''}`
                  )}
                </button>
              </div>
            </div>
          </div>
        </div>
      </>,
      document.body
    );
  }
);

function JiraIssueRow({
  issue,
  siteUrl,
  selected,
  onToggle,
}: {
  issue: JiraIssue;
  siteUrl: string;
  selected: boolean;
  onToggle: () => void;
}) {
  const priorityColor =
    issue.fields.priority?.name === 'Highest' ||
    issue.fields.priority?.name === 'High'
      ? 'text-error'
      : issue.fields.priority?.name === 'Medium'
        ? 'text-[hsl(35,80%,50%)]'
        : 'text-low';

  return (
    <div
      onClick={onToggle}
      className={`flex items-center gap-3 px-3 py-2 rounded cursor-pointer transition-colors ${
        selected ? 'bg-brand/10 border border-brand/30' : 'hover:bg-secondary'
      }`}
    >
      {/* Checkbox */}
      <div
        className={`w-4 h-4 rounded border flex items-center justify-center shrink-0 ${
          selected ? 'bg-brand border-brand text-white' : 'border-low'
        }`}
      >
        {selected && <CheckIcon size={10} weight="bold" />}
      </div>

      {/* Issue key */}
      <span className="text-xs font-mono text-low shrink-0 w-[80px]">
        {issue.key}
      </span>

      {/* Summary */}
      <span className="text-sm text-normal truncate flex-1">
        {issue.fields.summary}
      </span>

      {/* Priority */}
      {issue.fields.priority && (
        <span className={`text-xs shrink-0 ${priorityColor}`}>
          {issue.fields.priority.name}
        </span>
      )}

      {/* Status */}
      {issue.fields.status && (
        <span className="text-xs text-low shrink-0 px-1.5 py-0.5 bg-secondary rounded">
          {issue.fields.status.name}
        </span>
      )}

      {/* Open in Jira */}
      <a
        href={`${siteUrl}/browse/${issue.key}`}
        target="_blank"
        rel="noopener noreferrer"
        onClick={(e) => e.stopPropagation()}
        className="text-low hover:text-brand shrink-0"
        title="Open in Jira"
      >
        <ArrowSquareOutIcon size={14} />
      </a>
    </div>
  );
}

export const JiraImportDialog = defineModal<JiraImportDialogProps, void>(
  JiraImportDialogImpl
);
