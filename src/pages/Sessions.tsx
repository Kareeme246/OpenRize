import { BentoCard, BentoGrid, PageHeader } from "../components/Bento";

export function Sessions() {
  return (
    <main className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-5.5">
      <PageHeader
        title="Sessions"
        description="Timeline & timer · automatic tracking core, separate from manual Trackers"
        status="not-implemented"
      />

      <BentoGrid>
        <BentoCard
          span="wide"
          title="Timeline"
          description="Day-calendar view of every auto-captured session. Drag a range to add a Focus, Meeting, or Break block by hand."
        />
        <BentoCard
          span="md"
          title="Session timer"
          description="Radial countdown for whatever session is active, plus Start Session / Start Focus controls."
        />
        <BentoCard
          span="md"
          title="Automatic capture"
          description="Background polling of the active app, window title, and URL — every switch becomes a session with no manual start or stop."
        />
        <BentoCard
          span="md"
          title="Review panel"
          description="Docked panel for a pending entry: contributing apps, AI category suggestions with confidence scores, and task creation."
        />
        <BentoCard
          span="sm"
          title="Calendar scales"
          description="Day, Week, Month, and Year views of the same timeline."
        />
        <BentoCard
          span="sm"
          title="Idle detection"
          description="Stops tracking after a configurable period of inactivity and can start a Break automatically."
        />
      </BentoGrid>
    </main>
  );
}
