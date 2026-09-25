export function MyTimesheet() {
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
          <path d="M9 5H7a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-2" />
          <rect x="9" y="3" width="6" height="4" rx="1" />
          <path d="M9 14l2 2 4-4" />
        </svg>
      </div>
      <h2 className="text-lg font-bold text-fg">My Timesheet</h2>
      <p className="max-w-md text-xs text-fg-muted mt-1 leading-relaxed">
        My Timesheet allows you to review your submitted and approved hours by
        day, week, or month, and batch submit pending periods for client review.
      </p>
      <div className="mt-6 rounded-lg border border-line bg-surface/50 px-4 py-2 text-xs text-fg-soft font-mono">
        Phase 3 feature · Coming soon
      </div>
    </div>
  );
}
