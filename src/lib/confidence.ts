/**
 * Confidence bands, shown everywhere with both a label and a meter, never
 * colour alone. Thresholds mirror src-tauri/src/ai/mod.rs: at or above
 * `PREFILL` the AI pre-selects a value; below it the entry "Needs you".
 */

export type Band = "high" | "medium" | "low";

export const HIGH = 0.85;
export const PREFILL = 0.6;
export const METER_SEGMENTS = 5;

export function band(confidence: number): Band {
  if (confidence >= HIGH) return "high";
  if (confidence >= PREFILL) return "medium";
  return "low";
}

export const BAND_LABEL: Record<Band, string> = {
  high: "High",
  medium: "Medium",
  low: "Needs you",
};

/** Text colour per band: accent, neutral, and the review amber. */
export const BAND_TONE: Record<Band, string> = {
  high: "text-accent",
  medium: "text-fg-muted",
  low: "text-review",
};

export const BAND_FILL: Record<Band, string> = {
  high: "bg-accent",
  medium: "bg-fg-soft",
  low: "bg-review",
};

export function percent(confidence: number): string {
  return `${Math.round(confidence * 100)}%`;
}

/**
 * The confidence a Calendar block shows: the weaker of its fields, since an
 * entry only auto-approves when both clear the threshold.
 */
export function entryConfidence(
  category?: number,
  project?: number,
): number | undefined {
  const known = [category, project].filter(
    (value): value is number => value !== undefined,
  );
  return known.length === 0 ? undefined : Math.min(...known);
}
