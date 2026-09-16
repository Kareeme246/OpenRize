import { useState } from "react";
import { StopwatchCard } from "../components/StopwatchCard";
import type { TimersApi } from "../hooks/useTimers";

interface TrackersProps {
  api: TimersApi;
}

export function Trackers({ api }: TrackersProps) {
  const [label, setLabel] = useState("");
  const trimmed = label.trim();

  const submit = (): void => {
    if (trimmed.length === 0) return;
    api.create(trimmed);
    setLabel("");
  };

  return (
    <main className="flex min-h-0 flex-1 flex-col gap-3.5 overflow-y-auto p-5.5">
      <div className="flex gap-2.5">
        <input
          value={label}
          placeholder="What are you working on?"
          aria-label="New tracker name"
          onChange={(event) => setLabel(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") submit();
          }}
          className="min-w-0 flex-1 rounded-[10px] border border-white/10 bg-black/40 px-3.5 py-2.5 text-[13.5px] text-white placeholder:text-white/30 outline-none focus:border-accent/30"
        />
        <button
          type="button"
          onClick={submit}
          disabled={trimmed.length === 0}
          className="shrink-0 rounded-[10px] border border-accent/30 bg-linear-to-br from-accent to-accent-dim px-4 py-2.5 text-[13px] font-semibold text-[#04160c] disabled:opacity-40"
        >
          New tracker
        </button>
      </div>

      {api.error !== null && (
        <p
          role="alert"
          className="rounded-[10px] border border-red-400/40 bg-red-400/10 px-3.5 py-2.5 text-[13px] text-red-200"
        >
          {api.error}
        </p>
      )}

      {api.timers.length === 0 ? (
        <p className="rounded-xl border border-dashed border-white/10 px-4 py-10 text-center text-[13.5px] text-white/40">
          No trackers yet. Name one above, then hit Start to begin counting.
        </p>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-3">
          {api.timers.map((timer) => (
            <StopwatchCard
              key={timer.id}
              timer={timer}
              now={api.now}
              onStart={api.start}
              onPause={api.pause}
              onReset={api.reset}
              onRename={api.rename}
              onDelete={api.remove}
            />
          ))}
        </div>
      )}
    </main>
  );
}
