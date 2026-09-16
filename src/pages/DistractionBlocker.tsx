import { BentoCard, BentoGrid, PageHeader } from "../components/Bento";

export function DistractionBlocker() {
  return (
    <main className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-5.5">
      <PageHeader
        title="Distraction Blocker"
        description="Intervenes on distracting apps and sites during tracked sessions"
        status="not-implemented"
      />

      <BentoGrid>
        <BentoCard
          span="lg"
          title="Intervention prompt"
          description="On trigger: dismiss until the threshold hits again, mark the category as non-distracting for this session, disable the blocker, or jump to settings."
        />
        <BentoCard
          span="md"
          title="Blocking rules"
          description="Mark an app or site as a distraction and set how long a visit is allowed before it's interrupted."
        />
        <BentoCard
          span="sm"
          title="Session scoping"
          description="Only fires during session types blocking is enabled for — typically Focus."
        />
      </BentoGrid>
    </main>
  );
}
