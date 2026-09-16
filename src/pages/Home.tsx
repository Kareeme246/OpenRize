import { BentoCard, BentoGrid, PageHeader } from "../components/Bento";

export function Home() {
  return (
    <main className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-5.5">
      <PageHeader
        title="Home"
        description="Daily dashboard · lands here when the app opens"
        status="not-implemented"
      />

      <BentoGrid>
        <BentoCard
          span="lg"
          title="Time to review"
          description="Pending time entries waiting on a client, project, or task before they count toward anything."
        />
        <BentoCard
          span="md"
          title="Daily summary"
          description="Hours tracked, focus time, and a category breakdown for today."
        />
        <BentoCard
          span="md"
          title="Workspace hours"
          description="Tracked time across the team for the week. Team workspaces only."
        />
        <BentoCard
          span="sm"
          title="Quick links"
          description="Shortcuts to the pages you use most."
        />
        <BentoCard
          span="sm"
          title="Customize sidebar"
          description="Show, hide, and reorder nav items; set a workspace logo."
        />
      </BentoGrid>
    </main>
  );
}
