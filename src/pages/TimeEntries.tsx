export function TimeEntries() {
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
          <circle cx="12" cy="12" r="10" />
          <polyline points="12 6 12 12 16 14" />
        </svg>
      </div>
      <h2 className="text-lg font-bold text-fg">Time Entries</h2>
      <p className="max-w-md text-xs text-fg-muted mt-1 leading-relaxed">
        Comprehensive audit log and table view of all time entries with advanced
        filtering, grouping by project or client, and bulk export capabilities.
      </p>
      <div className="mt-6 rounded-lg border border-line bg-surface/50 px-4 py-2 text-xs text-fg-soft font-mono">
        Phase 3 feature · Use Calendar view to inspect and edit entries
      </div>
    </div>
  );
}
