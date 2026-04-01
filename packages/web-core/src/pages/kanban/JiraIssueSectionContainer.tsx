import { useState, useCallback, useRef, useEffect } from 'react';
import {
  SpinnerIcon,
  PaperPlaneRightIcon,
  ChatCircleDotsIcon,
  CheckCircleIcon,
} from '@phosphor-icons/react';
import { useJiraComments, useJiraPostSummary } from '@/shared/hooks/useJira';

interface JiraIssueSectionContainerProps {
  issueId: string;
}

export function JiraIssueSectionContainer({
  issueId,
}: JiraIssueSectionContainerProps) {
  const { data: comments, isLoading } = useJiraComments(issueId);
  const postSummary = useJiraPostSummary();
  const [posted, setPosted] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout>>();

  useEffect(() => () => clearTimeout(timerRef.current), []);

  const handlePostSummary = useCallback(async () => {
    try {
      await postSummary.mutateAsync(issueId);
      setPosted(true);
      timerRef.current = setTimeout(() => setPosted(false), 3000);
    } catch {
      // Error handled by mutation state
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [issueId]);

  return (
    <div className="px-base py-base">
      {/* Header */}
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-1.5 text-sm font-medium text-normal">
          <ChatCircleDotsIcon size={16} />
          Jira
        </div>
        <button
          type="button"
          onClick={handlePostSummary}
          disabled={postSummary.isPending}
          className="flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium text-brand hover:bg-brand/10 rounded border border-brand/30 transition-colors disabled:opacity-50"
        >
          {postSummary.isPending ? (
            <>
              <SpinnerIcon size={12} className="animate-spin" />
              Posting...
            </>
          ) : posted ? (
            <>
              <CheckCircleIcon size={12} />
              Posted!
            </>
          ) : (
            <>
              <PaperPlaneRightIcon size={12} />
              Post Summary to Jira
            </>
          )}
        </button>
      </div>

      {postSummary.isError && (
        <div className="text-xs text-error mb-2">
          {postSummary.error instanceof Error
            ? postSummary.error.message
            : 'Failed to post summary'}
        </div>
      )}

      {/* Comments list */}
      {isLoading ? (
        <div className="flex items-center gap-2 text-low text-xs py-2">
          <SpinnerIcon size={12} className="animate-spin" />
          Loading Jira comments...
        </div>
      ) : comments && comments.length > 0 ? (
        <div className="space-y-3">
          {comments.map((comment) => (
            <div key={comment.id} className="text-xs">
              <div className="flex items-center gap-1.5 mb-0.5">
                <span className="font-medium text-normal">
                  {comment.author_name}
                </span>
                {comment.created && (
                  <span className="text-low">
                    {new Date(comment.created).toLocaleDateString()}
                  </span>
                )}
              </div>
              <div className="text-low whitespace-pre-wrap leading-relaxed">
                {comment.body_markdown}
              </div>
            </div>
          ))}
        </div>
      ) : (
        <div className="text-xs text-low py-1">No Jira comments</div>
      )}
    </div>
  );
}
