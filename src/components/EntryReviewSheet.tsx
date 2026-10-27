import { useEffect, useState } from "react";
import type { EntryReview } from "../hooks/useEntryReview";
import { useSettings } from "../hooks/useSettings";
import { entryEnd, isLocked, isTyping } from "../lib/entries";
import { entryShortcut } from "../lib/entryShortcuts";
import { formatDuration, formatTime } from "../lib/format";
import {
  MERGE_GAP_MS,
  type MergeBlock,
  type MergeCandidate,
  mergeBlocks,
} from "../lib/merge";
import { shortcutLabel } from "../lib/platform";
import type { Category, Project } from "../lib/types";
import { ConfirmDialog } from "./ConfirmDialog";
import { EntryReviewPanel } from "./EntryReviewPanel";
import { SplitDialog } from "./SplitDialog";

interface EntryReviewSheetProps {
  review: EntryReview;
  categories: Category[];
  projects: Project[];
  /** "3 of 7" in review mode. */
  reviewPosition?: { index: number; total: number };
  /** Neighbouring entries a merge may take in, and the gap it would claim. */
  mergeCandidates?: MergeCandidate;
  /** Overrides approval, e.g. to advance review mode afterwards. */
  onAccept?: (id: string) => void;
  onReject?: (id: string) => void;
  onClose?: () => void;
  onManageCategories?: () => void;
  /**
   * The view draws entries on a timeline, so a split starts by picking the
   * cut there (see `useEntryReview`'s `splitting`) instead of in the dialog.
   */
  pickSplitOnTimeline?: boolean;
}

const MERGE_BLOCKED: Record<"prev" | "next", Record<MergeBlock, string>> = {
  prev: {
    none: `No earlier entry on the same day within ${formatDuration(MERGE_GAP_MS)} of this one.`,
    locked: "The earlier entry is approved. Unapprove it to merge.",
  },
  next: {
    none: `No later entry on the same day within ${formatDuration(MERGE_GAP_MS)} of this one.`,
    locked: "The later entry is approved. Unapprove it to merge.",
  },
};

/**
 * The entry review panel bound to `useEntryReview`, with its keyboard
 * contract: ⌘↵ accepts, ⌘⌫ rejects, S splits, Esc closes (or leaves a
 * split in progress). Merging has no shortcut. The panel itself owns 1–3, C, P,
 * and E. Renders nothing while no entry is open.
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
  onManageCategories,
  pickSplitOnTimeline = false,
}: EntryReviewSheetProps) {
  const { settings } = useSettings();
  const { detail } = review;
  const [confirmDelete, setConfirmDelete] = useState(false);
  const accept = onAccept ?? ((id: string) => void review.accept(id));
  const reject = onReject ?? ((id: string) => void review.reject(id));
  const close = onClose ?? (() => review.select(undefined));
  const prevId = mergeCandidates?.prev?.id;
  const nextId = mergeCandidates?.next?.id;
  const locked = detail ? isLocked(detail.entry) : false;
  const blocks = mergeBlocks(mergeCandidates, isLocked);
  const mergeBlocked = {
    prev: blocks.prev && MERGE_BLOCKED.prev[blocks.prev],
    next: blocks.next && MERGE_BLOCKED.next[blocks.next],
  };

  useEffect(() => {
    if (!detail) return;
    const onKeyDown = (event: KeyboardEvent): void => {
      if (isTyping(event.target)) return;
      const approved = detail.entry.status === "approved";
      const shortcut = entryShortcut(event);
      // The split dialog handles its own keys.
      if (review.splitting?.stage === "confirm") return;
      if (shortcut === "close") {
        if (review.splitting) review.cancelSplit();
        else close();
      } else if (shortcut === "accept") {
        event.preventDefault();
        if (detail.entry.categoryId && !approved) accept(detail.entry.id);
      } else if (shortcut === "reject") {
        event.preventDefault();
        if (!approved) reject(detail.entry.id);
      } else if (shortcut === "split") {
        event.preventDefault();
        if (!locked) review.beginSplit(pickSplitOnTimeline);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [detail, accept, reject, close, review, locked, pickSplitOnTimeline]);

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
          onSplit={() => review.beginSplit(pickSplitOnTimeline)}
          onMergePrev={prevId ? () => void review.merge(prevId) : undefined}
          onMergeNext={nextId ? () => void review.merge(nextId) : undefined}
          mergeBlocked={mergeBlocked}
          mergeGap={{
            prev: mergeCandidates?.prevGapMs,
            next: mergeCandidates?.nextGapMs,
          }}
          onDelete={() => setConfirmDelete(true)}
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
          onManageCategories={onManageCategories}
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
      {confirmDelete && (
        <ConfirmDialog
          title="Delete time entry?"
          body={`This entry, its captured activity, and its assigned category and project will be removed together. Other entries keep their time but lose these links. Undo with ${shortcutLabel("Z")}.`}
          confirmLabel="Delete"
          onCancel={() => setConfirmDelete(false)}
          onConfirm={() => {
            void review.remove();
            setConfirmDelete(false);
          }}
        />
      )}
    </div>
  );
}
