import { BentoCard, BentoGrid, PageHeader } from "../components/Bento";

export function AiAgent() {
  return (
    <main className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-5.5">
      <PageHeader
        title="AI Agent"
        description="Reports & Routines · conversational query layer over time data"
        status="not-implemented"
      />

      <BentoGrid>
        <BentoCard
          span="lg"
          title="Agent chat"
          description="Ask about time entries or profitability in plain English. Read-write: can also take manager actions, like approving entries."
        />
        <BentoCard
          span="md"
          title="AI reports"
          description="Template-based analysis that regenerates on a recurring schedule."
        />
        <BentoCard
          span="md"
          title="Routines"
          description="Scheduled, automated agent prompts that run without you asking."
        />
        <BentoCard
          span="sm"
          title="Agent context"
          description="Custom team-level instructions that steer how the agent reads your org's data."
        />
        <BentoCard
          span="sm"
          title="Shared AI skills"
          description="A reusable prompt library your team can share."
        />
      </BentoGrid>
    </main>
  );
}
