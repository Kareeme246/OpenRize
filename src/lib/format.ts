/**
 * Display formats shared by every view. Durations are `5h 22m` (design board,
 * D shared conventions), table hours are `3.4h`, and clock times follow the
 * user's locale.
 */

const HOUR_MS = 3_600_000;

/** `5h 02m`, `33m`, `<1m` for a few seconds, or `0m` for none. */
export function formatDuration(ms: number): string {
  if (ms > 0 && ms < 30_000) return "<1m";
  const minutes = Math.round(Math.max(0, ms) / 60_000);
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours === 0) return `${rest}m`;
  return `${hours}h ${String(rest).padStart(2, "0")}m`;
}

/** `3.4h` for pivots and cells; an en dash for nothing. */
export function formatHours(ms: number, empty = "–"): string {
  if (ms <= 0) return empty;
  const hours = ms / HOUR_MS;
  // A few minutes would round to "0.0h", which reads as nothing tracked.
  if (hours < 0.05) return "<0.1h";
  return `${hours >= 10 ? hours.toFixed(0) : hours.toFixed(1)}h`;
}

/** `9:38 AM` (or `09:38` in 24-hour locales). */
export function formatTime(epochMs: number): string {
  return new Date(epochMs).toLocaleTimeString([], {
    hour: "numeric",
    minute: "2-digit",
  });
}

/** `$4,620` in the given ISO currency, USD when none is set. */
export function formatMoney(amount: number, currency = "USD"): string {
  try {
    return new Intl.NumberFormat(undefined, {
      style: "currency",
      currency,
      maximumFractionDigits: amount >= 1000 ? 0 : 2,
    }).format(amount);
  } catch {
    return `${amount.toFixed(2)} ${currency}`;
  }
}

/** `12 min ago`, `Yesterday`, `Sep 21`, for "Last activity". */
export function formatRelative(epochMs: number, now: number): string {
  const minutes = Math.floor((now - epochMs) / 60_000);
  if (minutes < 1) return "Just now";
  if (minutes < 60) return `${minutes} min ago`;
  const then = new Date(epochMs);
  const today = new Date(now);
  today.setHours(0, 0, 0, 0);
  if (then.getTime() >= today.getTime()) {
    return `${Math.floor(minutes / 60)}h ago`;
  }
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  if (then.getTime() >= yesterday.getTime()) return "Yesterday";
  return formatShortDate(epochMs, now);
}

/** `Sep 21`, with the year only when it isn't this one. */
export function formatShortDate(epochMs: number, now = Date.now()): string {
  const date = new Date(epochMs);
  const sameYear = date.getFullYear() === new Date(now).getFullYear();
  return date.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    year: sameYear ? undefined : "numeric",
  });
}

export function plural(count: number, one: string, many = `${one}s`): string {
  return `${count} ${count === 1 ? one : many}`;
}
