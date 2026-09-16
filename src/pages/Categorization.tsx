import { BentoCard, BentoGrid, PageHeader } from "../components/Bento";

export function Categorization() {
  return (
    <main className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-5.5">
      <PageHeader
        title="Clients & Projects"
        description="Client → Project → Task categorization and rules"
        status="not-implemented"
      />

      <BentoGrid>
        <BentoCard
          span="lg"
          title="Client / project / task tree"
          description="The billing hierarchy every session and time entry gets tagged against."
        />
        <BentoCard
          span="md"
          title="Tracking rules"
          description="Map an app, website, or window-title pattern to a Client/Project/Task. The most specific matching rule wins."
        />
        <BentoCard
          span="md"
          title="AI auto-tagging"
          description="After ~2 minutes in Miscellaneous, suggests a category with a plain-English reason and a confidence score."
        />
        <BentoCard
          span="sm"
          title="Manual review queue"
          description="Accept or reject AI suggestions, or leave activity Uncategorized."
        />
        <BentoCard
          span="sm"
          title="Per-category settings"
          description="Toggle Focus scoring, Work Hours, idle timeout, and the Distraction Blocker per category."
        />
        <BentoCard
          span="sm"
          title="Team clients & projects"
          description="Org-level entries any member can tag time to. Team workspaces only."
        />
      </BentoGrid>
    </main>
  );
}
