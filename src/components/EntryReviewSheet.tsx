import { useEffect, useState } from "react";
import type { EntryReview } from "../hooks/useEntryReview";
import { useSettings } from "../hooks/useSettings";
import { entryEnd, isLocked, isTyping } from "../lib/entries";
import { formatDuration, formatTime } from "../lib/format";
import { shortcutLabel } from "../lib/platform";
import type {
  Category,
  Project,
  SuggestionField,
  TimeEntry,
} from "../lib/types";
import { ConfirmDialog } from "./ConfirmDialog";
import { EntryReviewPanel } from "./EntryReviewPanel";
import { SplitDialog } from "./SplitDialog";

interface EntryReviewSheetProps {
  review: EntryReview;
  categories: Category[];
  projects: Project[];
  /** "3 of 7" in review mode. */
  reviewPosition?: { index: number; total: number };
  /** Adjacent mergeable entries that start or end at the same border. */
  mergeCandidates?: { prev?: TimeEntry; next?: TimeEntry };
  /** Overrides approval, e.g. to advance review mode afterwards. */
  onAccept?: (id: string) => void;
  onReject?: (id: string) => void;
  onClose?: () => void;
  /**
   * The view draws entries on a timeline, so a split starts by picking the
   * cut there (see `useEntryReview`'s `splitting`) instead of in the dialog.
   */
  pickSplitOnTimeline?: boolean;
}

const NEIGHBOUR_LOCKED =
  "The adjacent entry is approved. Unapprove it to merge.";

/**
 * The entry review panel bound to `useEntryReview`, with its keyboard
 * contract: ⌘↵ accepts, ⌘⌫ rejects, S splits, M merges, Esc closes (or leaves
 * a split in progress). The panel itself
 * owns 1–9, C, P, and E. Renders nothing while no entry is open.
 */
export function EntryReviewSheet({
  review,
  categories,
  projects,
  reviewPosition,
  mergeCandidates,
  onAccept,
  onReject,
  onClose,
  pickSplitOnTimeline = false,
}: EntryReviewSheetProps) {
  const { settings } = useSettings();
  const { detail } = review;
  const [deleting, setDeleting] = useState<
    "entry" | { field: SuggestionField; id: string } | null
  >(null);
  const accept = onAccept ?? ((id: string) => void review.accept(id));
  const reject = onReject ?? ((id: string) => void review.reject(id));
  const close = onClose ?? (() => review.select(undefined));
  const prevId = mergeCandidates?.prev?.id;
  const nextId = mergeCandidates?.next?.id;
  const locked = detail ? isLocked(detail.entry) : false;
  const mergeBlocked = {
    prev:
      mergeCandidates?.prev && isLocked(mergeCandidates.prev)
        ? NEIGHBOUR_LOCKED
        : undefined,
    next:
      mergeCandidates?.next && isLocked(mergeCandidates.next)
        ? NEIGHBOUR_LOCKED
        : undefined,
  };

  useEffect(() => {
    if (!detail) return;
    const onKeyDown = (event: KeyboardEvent): void => {
      if (isTyping(event.target)) return;
      const command = event.metaKey || event.ctrlKey;
      const approved = detail.entry.status === "approved";
      // The split dialog handles its own keys.
      if (review.splitting?.stage === "confirm") return;
      if (event.key === "Escape") {
        if (review.splitting) review.cancelSplit();
        else close();
      } else if (command && event.key === "Enter") {
        event.preventDefault();
        if (detail.entry.categoryId && !approved) accept(detail.entry.id);
      } else if (command && event.key === "Backspace") {
        event.preventDefault();
        if (!approved) reject(detail.entry.id);
      } else if (!command && !event.altKey && event.key.toLowerCase() === "s") {
        event.preventDefault();
        if (!locked) review.beginSplit(pickSplitOnTimeline);
      } else if (!command && !event.altKey && event.key.toLowerCase() === "m") {
        if (locked) return;
        if (prevId && !mergeBlocked.prev) {
          event.preventDefault();
          void review.merge(prevId);
        } else if (nextId && !mergeBlocked.next) {
          event.preventDefault();
          void review.merge(nextId);
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    detail,
    accept,
    reject,
    close,
    review,
    prevId,
    nextId,
    locked,
    mergeBlocked.prev,
    mergeBlocked.next,
    pickSplitOnTimeline,
  ]);

  if (!detail) return null;
  return (
    <div data-toast-avoid className="flex h-full min-h-0 flex-col">
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
          onSplit={() => review.beginSplit(pickSplitOnTimeline)}
          canMerge={Boolean(prevId || nextId)}
          onMerge={
            prevId
              ? () => void review.merge(prevId)
              : nextId
                ? () => void review.merge(nextId)
                : undefined
          }
          onMergePrev={
            prevId && nextId ? () => void review.merge(prevId) : undefined
          }
          onMergeNext={
            prevId && nextId ? () => void review.merge(nextId) : undefined
          }
          mergeBlocked={mergeBlocked}
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
      {review.splitting?.stage === "confirm" && (
        <SplitDialog
          key={review.splitting.at}
          entry={detail.entry}
          initialAt={review.splitting.at}
          endsAt={entryEnd(detail.entry)}
          error={review.error}
          onCancel={review.cancelSplit}
          onSplit={(at) => void review.split(at)}
        />
      )}
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
