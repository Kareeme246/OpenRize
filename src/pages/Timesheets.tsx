export function Timesheets() {
  return (
    <div className="flex h-full min-h-0 flex-col items-center justify-center bg-base p-8 text-center">
      <div className="flex size-14 items-center justify-center rounded-2xl bg-surface border border-line text-accent mb-4">
        <svg
          viewBox="0 0 24 24"
          className="size-7"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.8}
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <rect x="3" y="3" width="18" height="18" rx="2" />
          <path d="M3 9h18M3 15h18M9 3v18M15 3v18" />
        </svg>
      </div>
      <h2 className="text-lg font-bold text-fg">Timesheets</h2>
      <p className="max-w-md text-xs text-fg-muted mt-1 leading-relaxed">
        Multi-user approval workflows, lock periods, team allocations, and
        organizational timesheet summaries across client accounts.
      </p>
      <div className="mt-6 rounded-lg border border-line bg-surface/50 px-4 py-2 text-xs text-fg-soft font-mono">
        Phase 5 feature · Coming soon
      </div>
    </div>
  );
}
