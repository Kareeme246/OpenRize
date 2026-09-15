/**
 * Mirror of `Timer` in src-tauri/src/timers.rs.
 * The Rust struct carries #[serde(rename_all = "camelCase")], so the JSON keys
 * arriving over IPC match these field names exactly.
 */
export interface Timer {
 id: string;
 label: string;
 accumulatedMs: number;
 startedAt: number | null;
 createdAt: number;
}

/**
 * The frontend needs elapsed time to draw a number; Rust owns the timestamps
 * that make it correct. The equivalent clamp on the Rust side is the
 * `saturating_sub` in `TimerStore::pause_at` — keep the two consistent if this
 * ever gains a second caller.
 */
export function elapsedMs(timer: Timer, now: number): number {
 if (timer.startedAt === null) return timer.accumulatedMs;
 return timer.accumulatedMs + Math.max(0, now - timer.startedAt);
}

/** `1:02:03` above an hour, `02:03` below it — never a width-jumping string. */
export function formatDuration(ms: number): string {
 const total = Math.max(0, Math.floor(ms / 1000));
 const hours = Math.floor(total / 3600);
 const minutes = Math.floor((total % 3600) / 60);
 const seconds = total % 60;
 const pad = (value: number): string => String(value).padStart(2, "0");
 return hours > 0
  ? `${hours}:${pad(minutes)}:${pad(seconds)}`
  : `${pad(minutes)}:${pad(seconds)}`;
}
