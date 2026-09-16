import { BentoCard, BentoGrid, PageHeader } from "../components/Bento";

export function Focus() {
  return (
    <main className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-5.5">
      <PageHeader
        title="Focus"
        description="Deep-work session type"
        status="not-implemented"
      />

      <BentoGrid>
        <BentoCard
          span="lg"
          title="Focus quality score"
          description="Scored across 20+ attributes during the session, surfacing context-switching and app-based distractions."
        />
        <BentoCard
          span="md"
          title="Automatic trigger"
          description="Starts a Focus session once ~75% of a 15+ minute window is spent in apps or sites rules mark as Focus."
        />
        <BentoCard
          span="sm"
          title="Calendar keyword trigger"
          description="Auto-starts from calendar events matching a keyword, e.g. #rize-focus."
        />
        <BentoCard
          span="sm"
          title="Manual start"
          description="Drag-select on the Sessions timeline, or hit Start Focus."
        />
        <BentoCard
          span="md"
          title="Focus rules"
          description="Choose which apps and websites count toward Focus mode."
        />
      </BentoGrid>
    </main>
  );
}
