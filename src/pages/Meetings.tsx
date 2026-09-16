import { BentoCard, BentoGrid, PageHeader } from "../components/Bento";

export function Meetings() {
  return (
    <main className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-5.5">
      <PageHeader
        title="Meetings"
        description="Calendar-driven session type"
        status="not-implemented"
      />

      <BentoGrid>
        <BentoCard
          span="lg"
          title="Meeting metrics"
          description="Meeting hours as a share of total tracked time, and meeting load per day or week."
        />
        <BentoCard
          span="md"
          title="Calendar detection"
          description="Event start and end times become session bounds automatically."
        />
        <BentoCard
          span="md"
          title="Keyword tagging"
          description="Scans event titles and descriptions to tag the right client or project."
        />
        <BentoCard
          span="sm"
          title="Manual meeting"
          description="Add a meeting by hand from the timeline."
        />
      </BentoGrid>
    </main>
  );
}
