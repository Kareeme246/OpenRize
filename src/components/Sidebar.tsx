export function Sidebar() {
  return (
    <aside className="flex min-h-0 flex-col gap-1.5 border-r border-white/10 bg-black/35 px-3.5 py-4.5">
      <div className="flex items-center gap-2.5 px-2 pb-4.5 pt-1">
        <div className="grid size-6.5 shrink-0 place-items-center rounded-lg bg-linear-to-br from-accent to-accent-dim shadow-[0_0_20px_rgba(42,234,131,0.35)]">
          <svg viewBox="0 0 24 24" className="size-3.5" fill="none" stroke="#04160c" strokeWidth={2.4} strokeLinecap="round" aria-hidden="true">
            <circle cx="12" cy="13" r="8" />
            <path d="M12 9v4.2l3 1.8M9 2h6" />
          </svg>
        </div>
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold tracking-tight">OpenRize</div>
          <div className="font-mono text-[10px] text-white/35">v0.1.0</div>
        </div>
      </div>

      {/* The only nav item in iteration 1. */}
      <div
        aria-current="page"
        className="flex items-center gap-2.5 rounded-[9px] border border-accent/30 bg-accent-soft px-2.5 py-2.5 text-[13.5px] text-accent"
      >
        <svg viewBox="0 0 24 24" className="size-3.5 shrink-0" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" aria-hidden="true">
          <circle cx="12" cy="13" r="8" />
          <path d="M12 9v4.2l3 1.8M9 2h6" />
        </svg>
        Trackers
      </div>

      <div className="mt-auto border-t border-white/10 px-2.5 pt-3 font-mono text-[10.5px] text-white/35">
        local only · no cloud
      </div>
    </aside>
  );
}
