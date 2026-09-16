import { BentoCard, BentoGrid, PageHeader } from "../components/Bento";

export function Breaks() {
  return (
    <main className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-5.5">
      <PageHeader
        title="Breaks"
        description="Rest-focused session type"
        status="not-implemented"
      />

      <BentoGrid>
        <BentoCard
          span="lg"
          title="Guided meditation"
          description="In-app guided meditations to use during a break."
        />
        <BentoCard
          span="md"
          title="Idle-triggered breaks"
          description="Starts automatically once idle time crosses its threshold."
        />
        <BentoCard
          span="sm"
          title="Manual break"
          description="Start a break on demand."
        />
        <BentoCard
          span="md"
          title="Break metrics"
          description="Break frequency and length compared against a target, to keep rest adequate."
        />
      </BentoGrid>
    </main>
  );
}
