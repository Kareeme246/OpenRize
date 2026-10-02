import type { AgentReport } from "../../lib/agents";
import { formatDuration } from "../../lib/format";
import type { Project } from "../../lib/types";

/**
 * Who did the time: your own, the agents' on top of it, and what bills.
 * Rust computes every figure under the Standard guardrails; the grid above
 * keeps showing only your own time.
 */
export function AgentSplit({
  report,
  projectById,
}: {
  report: AgentReport;
  projectById: Map<string, Project>;
}) {
  const rows = report.ledger.projects
    .filter((row) => row.youMs > 0 || row.agentMs > 0)
    .sort((a, b) => b.billableMs - a.billableMs);
  if (report.ledger.agentMs === 0 || rows.length === 0) return null;
  const pending = report.ledger.pendingMs;

  return (
    <section className="overflow-hidden rounded-xl border border-line bg-panel">
      <header className="flex items-baseline justify-between border-line border-b px-4 py-2.5">
        <h2 className="font-semibold text-[13px] text-fg-strong">
          You, agents, billable
        </h2>
        {pending > 0 && (
          <span className="text-[11.5px] text-review">
            {formatDuration(pending)} waiting for you to count it
          </span>
        )}
      </header>
      <table className="w-full text-[12px] tabular-nums">
        <thead>
          <tr className="text-left text-[11px] text-fg-soft uppercase tracking-wider">
            <th className="px-4 py-1.5 font-medium">Project</th>
            <th className="px-3 py-1.5 text-right font-medium">You</th>
            <th className="px-3 py-1.5 text-right font-medium">Agents</th>
            <th className="px-4 py-1.5 text-right font-medium">Billable</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => {
            const project = projectById.get(row.projectId);
            return (
              <tr key={row.projectId} className="border-line-soft border-t">
                <td className="px-4 py-2">
                  <span className="inline-flex items-center gap-2">
                    <i
                      className="size-2 rounded-full"
                      style={{
                        backgroundColor: project?.color ?? "var(--fg-faint)",
                      }}
                    />
                    {project?.name ?? "Unknown project"}
                  </span>
                </td>
                <td className="px-3 py-2 text-right">
                  {formatDuration(row.youMs)}
                </td>
                <td className="px-3 py-2 text-right text-fg-soft">
                  {row.agentMs > 0 ? `+${formatDuration(row.agentMs)}` : "–"}
                </td>
                <td className="px-4 py-2 text-right font-semibold text-fg-strong">
                  {formatDuration(row.billableMs)}
                </td>
              </tr>
            );
          })}
        </tbody>
        <tfoot>
          <tr className="border-line border-t font-semibold text-fg-strong">
            <td className="px-4 py-2">Total</td>
            <td className="px-3 py-2 text-right">
              {formatDuration(report.ledger.workMs)}
            </td>
            <td className="px-3 py-2 text-right text-fg-soft">
              +{formatDuration(report.ledger.agentMs)}
            </td>
            <td className="px-4 py-2 text-right">
              {formatDuration(report.ledger.billableMs)}
            </td>
          </tr>
        </tfoot>
      </table>
      <p className="border-line-soft border-t px-4 py-2 text-[11px] text-fg-faint">
        Work hours stay your own time. Agent time bills only while you were
        working, supervised, and no more than three projects ran at once.
      </p>
    </section>
  );
}
