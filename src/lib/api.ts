import { invoke } from "@tauri-apps/api/core";
import type { Timer } from "./timers";

/** Rust emits this after every mutation; see useTimers. */
export const TIMERS_CHANGED = "timers-changed";

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

/** Tauri rejects with a string; React errors are Error objects. Handle both. */
export function describeError(error: unknown): string {
 if (typeof error === "string") return error;
 if (error instanceof Error) return error.message;
 return String(error);
}
