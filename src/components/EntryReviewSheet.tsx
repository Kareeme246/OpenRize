import { useEffect } from "react";
import type { EntryReview } from "../hooks/useEntryReview";
import { useSettings } from "../hooks/useSettings";
import { isTyping } from "../lib/entries";
import { formatDuration, formatTime } from "../lib/format";
import type { Category, Project } from "../lib/types";
import { EntryReviewPanel } from "./EntryReviewPanel";

interface EntryReviewSheetProps {
  review: EntryReview;
  categories: Category[];
  projects: Project[];
  /** "3 of 7" in review mode. */
  reviewPosition?: { index: number; total: number };
  /** Overrides approval, e.g. to advance review mode afterwards. */
  onAccept?: (id: string) => void;
  onReject?: (id: string) => void;
  onClose?: () => void;
}

/**
 * The entry review panel bound to `useEntryReview`, with its keyboard
 * contract: ⌘↵ accepts, ⌘⌫ rejects, S splits, Esc closes. The panel itself
 * owns 1–9, C, P, and E. Renders nothing while no entry is open.
 */
export function EntryReviewSheet({
  review,
  categories,
  projects,
  reviewPosition,
  onAccept,
  onReject,
  onClose,
}: EntryReviewSheetProps) {
  const { settings } = useSettings();
  const { detail } = review;
  const accept = onAccept ?? ((id: string) => void review.accept(id));
  const reject = onReject ?? ((id: string) => void review.reject(id));
  const close = onClose ?? (() => review.select(undefined));

  useEffect(() => {
    if (!detail) return;
    const onKeyDown = (event: KeyboardEvent): void => {
      if (isTyping(event.target)) return;
      const command = event.metaKey || event.ctrlKey;
      const approved = detail.entry.status === "approved";
      if (event.key === "Escape") {
        close();
      } else if (command && event.key === "Enter") {
        event.preventDefault();
        if (detail.entry.categoryId && !approved) accept(detail.entry.id);
      } else if (command && event.key === "Backspace") {
        event.preventDefault();
        if (!approved) reject(detail.entry.id);
      } else if (!command && !event.altKey && event.key.toLowerCase() === "s") {
        event.preventDefault();
        void review.split();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [detail, accept, reject, close, review]);

  if (!detail) return null;
  return (
    <div className="flex h-full min-h-0 flex-col">
      {review.error && (
        <div
          role="alert"
          className="border-danger/30 border-b bg-danger-soft px-4 py-2 text-[11.5px] text-danger"
        >
          {review.error}
        </div>
      )}
      <div className="min-h-0 flex-1">
        <EntryReviewPanel
          detail={detail}
          categories={categories}
          projects={projects}
          suggestProjects={settings.aiSuggest === "categoryProject"}
          reviewPosition={reviewPosition}
          onClose={close}
          onAccept={() => accept(detail.entry.id)}
          onReject={() => reject(detail.entry.id)}
          onSplit={() => void review.split()}
          onDelete={() => void review.remove()}
          onRetry={() => void review.retry()}
          onSetField={(field, valueId) => void review.setField(field, valueId)}
          onToggleBillable={() => void review.toggleBillable()}
          onSaveDescription={(text) => void review.saveDescription(text)}
          onResolveRule={(suggestion, acceptRule) =>
            void review.resolveRule(suggestion, acceptRule)
          }
          formatTime={formatTime}
          formatDuration={formatDuration}
        />
      </div>
    </div>
  );
}
