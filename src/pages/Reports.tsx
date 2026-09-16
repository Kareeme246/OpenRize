import { BentoCard, BentoGrid, PageHeader } from "../components/Bento";

export function Reports() {
  return (
    <main className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-5.5">
      <PageHeader
        title="Dashboards & Reports"
        description="Analytics layer over sessions, categorization, and time entries"
        status="not-implemented"
      />

      <BentoGrid>
        <BentoCard
          span="lg"
          title="Custom dashboards"
          description="Start from a template or a blank canvas, then drag widgets into a layout. Unlimited dashboards, filterable by day/week/month/custom range."
        />
        <BentoCard
          span="md"
          title="Starter templates"
          description="Team Overview, Clients Overview, and Personal Productivity, ready to customize."
        />
        <BentoCard
          span="md"
          title="Profitability"
          description="Real-time margin per project from tracked time, cost rates, and contract value; budget burn and effective hourly rate."
        />
        <BentoCard
          span="sm"
          title="Resourcing"
          description="Hours by member, project, and client to spot who's overloaded vs. who has spare capacity."
        />
        <BentoCard
          span="sm"
          title="Timesheets"
          description="Personal and team hour review, built from auto-captured and AI-tagged entries."
        />
        <BentoCard
          span="sm"
          title="Team analytics"
          description="Org-wide utilization, aggregated across everyone."
        />
      </BentoGrid>
    </main>
  );
}
