import { useMemo, useState } from "react";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { FIELD, TEXTAREA_FIELD } from "../../components/Page";
import { Toggle } from "../../components/SegmentedControl";
import { Field, Sheet } from "../../components/Sheet";
import { useToast } from "../../hooks/useToast";
import * as api from "../../lib/api";
import { describeError } from "../../lib/api";
import { plural } from "../../lib/format";
import { PALETTE } from "../../lib/palette";
import type {
  Category,
  CategoryRemoval,
  CategoryStats,
  NewCategory,
  UpdateCategory,
} from "../../lib/types";

interface CategorySheetProps {
  /** Editing an existing category; absent creates one. */
  category?: Category;
  /** Existing categories, used to pick the least-used palette swatch. */
  categories?: Category[];
  /** Usage stats across all entries for smart delete. */
  stats?: CategoryStats;
  onClose: () => void;
  onSaved: (category: Category) => void;
  onRemoved?: (category: Category, removal: CategoryRemoval) => void;
}

/** Pick the swatch least used by live categories. */
function leastUsedSwatch(
  categories: Category[] = [],
  exceptId?: string,
): string {
  const counts = new Map<string, number>();
  for (const swatch of PALETTE) {
    counts.set(swatch.toLowerCase(), 0);
  }
  for (const cat of categories) {
    if (cat.archived || cat.deletedAt || cat.id === exceptId) continue;
    const key = cat.color.toLowerCase();
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  let best = PALETTE[0];
  let min = Infinity;
  for (const swatch of PALETTE) {
    const c = counts.get(swatch.toLowerCase()) ?? 0;
    if (c < min) {
      min = c;
      best = swatch;
    }
  }
  return best;
}

/**
 * Category editor sheet: name, palette swatch, "what counts" description,
 * optional apps & sites hints, work hours toggle, and smart delete/archive.
 */
export function CategorySheet({
  category,
  categories = [],
  stats,
  onClose,
  onSaved,
  onRemoved,
}: CategorySheetProps) {
  const showToast = useToast();
  const [name, setName] = useState(category?.name ?? "");
  const [color, setColor] = useState(() =>
    category ? category.color : leastUsedSwatch(categories),
  );
  const [description, setDescription] = useState(category?.description ?? "");
  const [hints, setHints] = useState(category?.aiHints ?? "");
  const [countsAsWork, setCountsAsWork] = useState(
    category?.countsAsWork ?? true,
  );
  const [aiPrompt, setAiPrompt] = useState<string | null>(
    category?.aiPrompt ?? null,
  );
  const [showHints, setShowHints] = useState(Boolean(category?.aiHints));
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const entriesCount = stats?.entries ?? 0;
  const isUsed = entriesCount > 0;

  const valid = name.trim().length > 0;

  const placeholder = useMemo(() => {
    const trimmed = name.trim();
    if (!trimmed)
      return "e.g. Sales calls, CRM updates, proposals, customer outreach";
    return `e.g. Work that counts as ${trimmed}`;
  }, [name]);

  const submit = async (): Promise<void> => {
    if (!valid || saving) return;
    setSaving(true);
    setError(null);
    try {
      if (category) {
        const patch: UpdateCategory = {
          name: name.trim() !== category.name ? name.trim() : undefined,
          color: color !== category.color ? color : undefined,
          description:
            description.trim() !== (category.description ?? "")
              ? description.trim() || null
              : undefined,
          aiHints:
            hints.trim() !== (category.aiHints ?? "")
              ? hints.trim() || null
              : undefined,
          countsAsWork:
            countsAsWork !== category.countsAsWork ? countsAsWork : undefined,
          aiPrompt: aiPrompt !== category.aiPrompt ? aiPrompt : undefined,
        };
        const saved = await api.updateCategory(category.id, patch);
        showToast({
          message: `Saved "${saved.name}"`,
          tone: "success",
        });
        onSaved(saved);
        onClose();
      } else {
        const newCat: NewCategory = {
          name: name.trim(),
          color,
          description: description.trim() || undefined,
          aiHints: hints.trim() || undefined,
          countsAsWork,
        };
        const created = await api.createCategory(newCat);
        showToast({
          message: `Created "${created.name}"`,
          tone: "success",
        });
        onSaved(created);
        onClose();
      }
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setSaving(false);
    }
  };

  const executeRemove = async (): Promise<void> => {
    if (!category) return;
    setConfirmDelete(false);
    try {
      const removal = await api.removeCategory(category.id);
      showToast({
        message:
          removal === "archived"
            ? `"${category.name}" archived`
            : `Deleted "${category.name}"`,
        tone: "success",
      });
      onRemoved?.(category, removal);
      onClose();
    } catch (cause) {
      setError(describeError(cause));
    }
  };

  return (
    <>
      <Sheet
        title={category ? `Edit ${category.name}` : "New category"}
        submitLabel={saving ? "Saving…" : category ? "Save" : "Create category"}
        canSubmit={valid && !saving}
        onSubmit={submit}
        onClose={onClose}
        error={error}
        width="md"
      >
        <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-3">
          <Field label="Name" htmlFor="category-name">
            <input
              id="category-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              // biome-ignore lint/a11y/noAutofocus: sheet opens to edit name
              autoFocus
              placeholder="e.g. Sales"
              className={FIELD}
              required
            />
          </Field>
          <Field label="Colour">
            <div className="flex h-8 items-center gap-1">
              {PALETTE.map((swatch) => (
                <label
                  key={swatch}
                  className={`size-5 cursor-pointer rounded-full transition-transform has-focus-visible:ring-2 has-focus-visible:ring-accent ${
                    color.toLowerCase() === swatch.toLowerCase()
                      ? "scale-110 ring-2 ring-fg-strong ring-offset-2 ring-offset-panel"
                      : "hover:scale-110"
                  }`}
                  style={{ backgroundColor: swatch }}
                >
                  <input
                    type="radio"
                    name="category-colour"
                    value={swatch}
                    checked={color.toLowerCase() === swatch.toLowerCase()}
                    onChange={() => setColor(swatch)}
                    aria-label={`Colour ${swatch}`}
                    className="sr-only"
                  />
                </label>
              ))}
            </div>
          </Field>
        </div>

        <Field
          label={`What counts as ${name.trim() || "this category"}?`}
          htmlFor="category-description"
          hint="One line describing the kind of work. Apple Intelligence uses this to suggest this category."
        >
          <textarea
            id="category-description"
            rows={2}
            value={description}
            onChange={(event) => setDescription(event.target.value)}
            placeholder={placeholder}
            className={TEXTAREA_FIELD}
          />
        </Field>

        <div>
          {!showHints ? (
            <button
              type="button"
              onClick={() => setShowHints(true)}
              className="text-[11.5px] font-medium text-accent hover:underline"
            >
              + Add apps & sites rules
            </button>
          ) : (
            <Field
              label="Apps & sites"
              htmlFor="category-hints"
              hint="Keywords, domain names, and folder paths to always match (comma-separated)."
            >
              <textarea
                id="category-hints"
                rows={2}
                value={hints}
                onChange={(event) => setHints(event.target.value)}
                placeholder="e.g. salesforce.com, hubspot, /Sales"
                className={TEXTAREA_FIELD}
              />
            </Field>
          )}
        </div>

        <div className="flex items-center justify-between rounded-lg border border-line bg-surface/50 p-3">
          <div>
            <div className="font-medium text-[12px] text-fg-strong">
              Count toward work hours
            </div>
            <div className="text-[11px] text-fg-faint">
              Include in daily and weekly work totals on Pulse and Calendar.
            </div>
          </div>
          <Toggle
            checked={countsAsWork}
            onChange={setCountsAsWork}
            label="Count toward work hours"
          />
        </div>

        {aiPrompt && (
          <div className="rounded-lg border border-line bg-surface/30 p-3">
            <div className="flex items-center justify-between">
              <span className="font-medium text-[11px] text-fg-soft">
                Built-in prompt
              </span>
              <button
                type="button"
                onClick={() => setAiPrompt(null)}
                className="text-[11px] text-fg-faint hover:text-danger"
              >
                Clear
              </button>
            </div>
            <p className="mt-1 text-[11px] text-fg-muted italic">{aiPrompt}</p>
          </div>
        )}

        {category && (
          <div className="pt-2 border-t border-line">
            <button
              type="button"
              onClick={() => setConfirmDelete(true)}
              className="text-[12px] text-danger hover:underline"
            >
              {isUsed ? "Archive category…" : "Delete category…"}
            </button>
          </div>
        )}
      </Sheet>

      {confirmDelete && category && (
        <ConfirmDialog
          title={
            isUsed
              ? `Archive "${category.name}"?`
              : `Delete "${category.name}"?`
          }
          body={
            isUsed
              ? `"${category.name}" is on ${plural(entriesCount, "entry")} (${(
                  (stats?.totalMs ?? 0) / 3_600_000
                ).toFixed(
                  1,
                )}h). It will be hidden from pickers and suggestions; past entries keep it. Restore anytime from the Archived tab.`
              : `"${category.name}" has never been used. It will be permanently removed. Undo with ⌘Z.`
          }
          confirmLabel={isUsed ? "Archive" : "Delete"}
          tone="destructive"
          onCancel={() => setConfirmDelete(false)}
          onConfirm={() => void executeRemove()}
        />
      )}
    </>
  );
}
