export function Invoices() {
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
          <rect x="4" y="2" width="16" height="20" rx="2" />
          <line x1="8" y1="6" x2="16" y2="6" />
          <line x1="8" y1="10" x2="16" y2="10" />
          <line x1="8" y1="14" x2="12" y2="14" />
        </svg>
      </div>
      <h2 className="text-lg font-bold text-fg">Invoices</h2>
      <p className="max-w-md text-xs text-fg-muted mt-1 leading-relaxed">
        Generate and export client invoices directly from approved billable time
        entries, configured hourly rates, and fixed-price project milestones.
      </p>
      <div className="mt-6 rounded-lg border border-line bg-surface/50 px-4 py-2 text-xs text-fg-soft font-mono">
        Phase 5 feature · Coming soon
      </div>
    </div>
  );
}
