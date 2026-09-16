import { BentoCard, BentoGrid, PageHeader } from "../components/Bento";

export function Integrations() {
  return (
    <main className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-5.5">
      <PageHeader
        title="Integrations"
        description="Connections to task tools, accounting, calendars, and the outside world"
        status="not-implemented"
      />

      <BentoGrid>
        <BentoCard
          span="md"
          title="Task & PM tools"
          description="ClickUp, Linear, Asana — used as AI-tagging signal and for creating tasks straight from the review panel."
        />
        <BentoCard
          span="md"
          title="Accounting exports"
          description="FreshBooks, QuickBooks, and Xero via Zapier, or a plain CSV. OpenRize doesn't generate invoices itself."
        />
        <BentoCard
          span="md"
          title="Calendar sync"
          description="Powers Meeting detection and Focus keyword scanning."
        />
        <BentoCard
          span="sm"
          title="Webhooks"
          description="Push session and entry events to your own endpoint."
        />
        <BentoCard
          span="sm"
          title="Public API"
          description="A GraphQL surface over the same data the in-app agent and MCP server use."
        />
      </BentoGrid>
    </main>
  );
}
