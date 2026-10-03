import { useEffect, useState } from "react";
import type { EntryReview } from "../hooks/useEntryReview";
import { useSettings } from "../hooks/useSettings";
import { isTyping } from "../lib/entries";
import { formatDuration, formatTime } from "../lib/format";
import { shortcutLabel } from "../lib/platform";
import type { Category, Project, SuggestionField } from "../lib/types";
import { ConfirmDialog } from "./ConfirmDialog";
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
  const [deleting, setDeleting] = useState<
    "entry" | { field: SuggestionField; id: string } | null
  >(null);
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
          onUnapprove={() => void review.unapprove()}
          onSplit={() => void review.split()}
          onDelete={() => setDeleting("entry")}
          onDeleteField={(field, id) => setDeleting({ field, id })}
          onRetry={() => void review.retry()}
          onSetField={(field, valueId) => void review.setField(field, valueId)}
          onToggleBillable={() => void review.toggleBillable()}
          onSaveDescription={(text) => void review.saveDescription(text)}
          onSetTimes={(startedAt, endedAt) =>
            void review.setTimes(startedAt, endedAt)
          }
          onResolveRule={(suggestion, acceptRule) =>
            void review.resolveRule(suggestion, acceptRule)
          }
          formatTime={formatTime}
          formatDuration={formatDuration}
        />
      </div>
      {deleting && (
        <ConfirmDialog
          title={`Delete ${deleting === "entry" ? "time entry" : deleting.field}?`}
          body={
            deleting === "entry"
              ? `This entry, its captured activity, and its assigned category and project will be removed together. Other entries keep their time but lose these links. Undo with ${shortcutLabel("Z")}.`
              : `This ${deleting.field} and its rules will be removed. Other entries keep their time but lose this link. Undo with ${shortcutLabel("Z")}.`
          }
          confirmLabel="Delete"
          onCancel={() => setDeleting(null)}
          onConfirm={() => {
            if (deleting === "entry") void review.remove();
            else void review.removeField(deleting.field, deleting.id);
            setDeleting(null);
          }}
        />
      )}
    </div>
  );
}
