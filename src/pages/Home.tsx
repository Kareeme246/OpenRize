import { AppBreakdown, CardBody, Stat } from "../components/ActivityBits";
import { BentoCard, BentoGrid, PageHeader } from "../components/Bento";
import { useNotImplemented } from "../components/NotImplemented";
import { useActivity } from "../hooks/useActivity";
import { segmentDuration } from "../lib/activity";
import { formatDuration } from "../lib/timers";

export function Home() {
  const activity = useActivity();
  const { show } = useNotImplemented();
  const snapshot = activity.snapshot;
  const segments = snapshot?.segments ?? [];
  const now = activity.now;

  // "To review" is any auto-captured entry the user has not confirmed. Until
  // categorization exists, confirming is the whole review step.
  const unreviewed = segments.filter(
    (segment) => !segment.reviewed && segment.kind === "activity",
  );
  const current = snapshot?.current ?? null;

  return (
    <main className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-5.5">
      <PageHeader
        title="Home"
        description="Daily dashboard · lands here when the app opens"
        status="live"
      />

      {activity.error !== null && (
        <p
          role="alert"
          className="rounded-[10px] border border-danger/40 bg-danger-soft px-3.5 py-2.5 text-[13px] text-danger"
        >
          {activity.error}
        </p>
      )}

      <BentoGrid>
        <BentoCard
          span="lg"
          title="Daily summary"
          description="Hours tracked today, how much of it was Focus, and the apps they went to."
          status="live"
        >
          <div className="flex gap-6">
            <Stat
              label="Tracked"
              value={formatDuration(snapshot?.trackedMs ?? 0)}
            />
            <Stat
              label="Focus"
              value={formatDuration(snapshot?.focusMs ?? 0)}
            />
            <Stat
              label="Break"
              value={formatDuration(snapshot?.breakMs ?? 0)}
            />
          </div>
          <CardBody>
            <AppBreakdown segments={segments} now={now} />
          </CardBody>
        </BentoCard>

        <BentoCard
          span="md"
          title="Time to review"
          description="Auto-captured entries you have not confirmed yet."
          status="live"
        >
          {unreviewed.length === 0 ? (
            <p className="rounded-lg border border-dashed border-line px-3 py-6 text-center text-[12px] text-fg-faint">
              Everything is reviewed.
            </p>
          ) : (
            <CardBody>
              <div className="flex flex-col gap-1">
                {unreviewed.slice(0, 10).map((segment) => (
                  <div
                    key={segment.id}
                    className="flex items-center gap-2 rounded-lg px-2 py-1.5 hover:bg-surface"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-[12.5px] text-fg">
                        {segment.title.length > 0 ? segment.title : segment.app}
                      </div>
                      <div className="font-mono text-[10.5px] text-fg-faint">
                        {formatDuration(segmentDuration(segment, now))}
                      </div>
                    </div>
                    <button
                      type="button"
                      onClick={() => activity.markReviewed(segment.id)}
                      className="shrink-0 rounded-md border border-line bg-surface px-2 py-1 text-[11px] text-fg-muted hover:bg-surface-strong"
                    >
                      Review
                    </button>
                  </div>
                ))}
              </div>
            </CardBody>
          )}
        </BentoCard>

        <BentoCard
          span="md"
          title="Right now"
          description="The live capture state, straight from the sampler."
          status="live"
        >
          <div className="flex flex-col gap-2">
            <Stat
              label="Active window"
              value={current === null ? "Nothing" : current.app}
              hint={
                current !== null && current.title.length > 0
                  ? current.title
                  : "No window title captured"
              }
            />
            <div className="flex items-center gap-2 font-mono text-[10.5px] text-fg-faint">
              <span
                className={`size-1.5 rounded-full ${
                  snapshot?.captureEnabled ? "bg-accent" : "bg-fg-ghost"
                }`}
              />
              {snapshot?.captureEnabled ? "Capture running" : "Capture paused"}{" "}
              · idle {formatDuration(snapshot?.idleMs ?? 0)}
            </div>
          </div>
        </BentoCard>

        <BentoCard
          span="md"
          title="Workspace hours"
          description="Tracked time across the team for the week. Team workspaces only."
        >
          <button
            type="button"
            onClick={() => show("Workspace hours")}
            className="self-start rounded-lg border border-line bg-surface px-3 py-1.5 text-[12px] text-fg-muted hover:bg-surface-strong"
          >
            Open workspaces
          </button>
        </BentoCard>
      </BentoGrid>
    </main>
  );
}
