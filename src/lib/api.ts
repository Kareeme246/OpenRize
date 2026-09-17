import { invoke } from "@tauri-apps/api/core";
import type { ActivitySnapshot, SessionKind } from "./activity";
import type { Settings, StoragePaths } from "./settings";
import type { Timer } from "./timers";

/** Rust emits this after every mutation; see useTimers. */
export const TIMERS_CHANGED = "timers-changed";

/**
 * Rust emits this whenever a segment actually opens or closes, and once more
 * immediately when the OpenRize window regains focus (a reconciliation).
 * Carries the full `ActivitySnapshot` directly — no follow-up call needed.
 */
export const ACTIVITY_CHANGED = "activity-changed";

/**
 * Rust emits this at 1Hz while the OpenRize window is focused, or every 30s
 * while it isn't and nothing structural has changed. Carries the lighter
 * `ActivityTick` (same numbers, no segment list).
 */
export const ACTIVITY_TICK = "activity-tick";

/** Rust emits this after a preference change so every view adopts it. */
export const SETTINGS_CHANGED = "settings-changed";

/*
 * One typed wrapper per #[tauri::command]. Every command returns the complete
 * timer list, so callers never have to guess at state they did not just write.
 * Tauri rejects with the Rust Err value, which is a plain string.
 */
export function listTimers(): Promise<Timer[]> {
  return invoke<Timer[]>("list_timers");
}

export function createTimer(label: string): Promise<Timer[]> {
  return invoke<Timer[]>("create_timer", { label });
}

export function startTimer(id: string): Promise<Timer[]> {
  return invoke<Timer[]>("start_timer", { id });
}

export function pauseTimer(id: string): Promise<Timer[]> {
  return invoke<Timer[]>("pause_timer", { id });
}

export function resetTimer(id: string): Promise<Timer[]> {
  return invoke<Timer[]>("reset_timer", { id });
}

export function renameTimer(id: string, label: string): Promise<Timer[]> {
  return invoke<Timer[]>("rename_timer", { id, label });
}

export function deleteTimer(id: string): Promise<Timer[]> {
  return invoke<Timer[]>("delete_timer", { id });
}

// --- activity capture -------------------------------------------------

export function activitySnapshot(sinceMs: number): Promise<ActivitySnapshot> {
  return invoke<ActivitySnapshot>("activity_snapshot", { sinceMs });
}

export function setCaptureEnabled(enabled: boolean): Promise<void> {
  return invoke<void>("set_capture_enabled", { enabled });
}

export function setIdleThreshold(minutes: number): Promise<void> {
  return invoke<void>("set_idle_threshold", { minutes });
}

export function startSession(kind: SessionKind, label?: string): Promise<void> {
  return invoke<void>("start_session", { kind, label: label ?? null });
}

export function stopSession(): Promise<void> {
  return invoke<void>("stop_session");
}

export function markSegmentReviewed(id: number): Promise<void> {
  return invoke<void>("mark_segment_reviewed", { id });
}

// --- preferences ------------------------------------------------------

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

/** Replaces the whole preference set; Rust answers with the stored values. */
export function updateSettings(settings: Settings): Promise<Settings> {
  return invoke<Settings>("update_settings", { settings });
}

export function storagePaths(): Promise<StoragePaths> {
  return invoke<StoragePaths>("storage_paths");
}

/** Tauri rejects with a string; React errors are Error objects. Handle both. */
export function describeError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
