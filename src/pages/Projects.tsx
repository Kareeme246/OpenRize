import { useCallback, useEffect, useState } from "react";
import * as api from "../lib/api";
import type { Client, NewClient, NewProject, Project } from "../lib/types";

export function Projects() {
  const [tab, setTab] = useState<
    "active" | "completed" | "archived" | "clients"
  >("active");
  const [projects, setProjects] = useState<Project[]>([]);
  const [clients, setClients] = useState<Client[]>([]);
  const [showNewProject, setShowNewProject] = useState(false);
  const [showNewClient, setShowNewClient] = useState(false);

  // Form states
  const [newProjName, setNewProjName] = useState("");
  const [newProjColor, setNewProjColor] = useState("#75a4e5");
  const [newProjClientId, setNewProjClientId] = useState<string>("");
  const [newProjHints, setNewProjHints] = useState("");
  const [newProjRate, setNewProjRate] = useState<number | undefined>(undefined);
  const [newProjBudgetKind, setNewProjBudgetKind] = useState("none");
  const [newProjBudgetValue, setNewProjBudgetValue] = useState<
    number | undefined
  >(undefined);

  const [newClientName, setNewClientName] = useState("");
  const [newClientEmail, setNewClientEmail] = useState("");
  const [newClientRate, setNewClientRate] = useState<number | undefined>(
    undefined,
  );

  const loadData = useCallback(async () => {
    try {
      const [projs, cls] = await Promise.all([
        api.listProjects(),
        api.listClients(),
      ]);
      setProjects(projs);
      setClients(cls);
    } catch (err) {
      console.error("Failed to load projects/clients", err);
    }
  }, []);

  useEffect(() => {
    loadData();
  }, [loadData]);

  const handleCreateProject = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newProjName.trim()) return;

    try {
      const payload: NewProject = {
        name: newProjName.trim(),
        color: newProjColor,
        clientId: newProjClientId ? newProjClientId : undefined,
        aiHints: newProjHints ? newProjHints : undefined,
        hourlyRate: newProjRate,
        budgetKind: newProjBudgetKind,
        budgetValue: newProjBudgetValue,
      };
      await api.createProject(payload);
      setNewProjName("");
      setNewProjHints("");
      setNewProjRate(undefined);
      setNewProjBudgetValue(undefined);
      setShowNewProject(false);
      await loadData();
    } catch (err) {
      console.error("Failed to create project", err);
    }
  };

  const handleCreateClient = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newClientName.trim()) return;

    try {
      const payload: NewClient = {
        name: newClientName.trim(),
        email: newClientEmail ? newClientEmail.trim() : undefined,
        defaultRate: newClientRate,
      };
      await api.createClient(payload);
      setNewClientName("");
      setNewClientEmail("");
      setNewClientRate(undefined);
      setShowNewClient(false);
      await loadData();
    } catch (err) {
      console.error("Failed to create client", err);
    }
  };

  const clientMap = new Map<string, string>();
  for (const c of clients) {
    clientMap.set(c.id, c.name);
  }

  const filteredProjects = projects.filter((p) => {
    if (tab === "active") return p.status === "active";
    if (tab === "completed") return p.status === "completed";
    if (tab === "archived") return p.status === "archived";
    return true;
  });

  return (
    <div className="flex h-full flex-col min-h-0 overflow-hidden bg-canvas text-fg">
      <header className="flex h-12 shrink-0 items-center justify-between border-b border-line px-5">
        <div className="flex items-center gap-3">
          <h1 className="text-[15px] font-semibold text-fg-strong">Projects</h1>
          <div className="flex rounded-md border border-line bg-panel p-0.5 text-[12px] font-medium">
            <button
              type="button"
              onClick={() => setTab("active")}
              className={`rounded px-2.5 py-0.5 transition-colors ${
                tab === "active"
                  ? "bg-accent/20 text-accent font-semibold"
                  : "text-fg-soft hover:text-fg"
              }`}
            >
              Active ({projects.filter((p) => p.status === "active").length})
            </button>
            <button
              type="button"
              onClick={() => setTab("completed")}
              className={`rounded px-2.5 py-0.5 transition-colors ${
                tab === "completed"
                  ? "bg-accent/20 text-accent font-semibold"
                  : "text-fg-soft hover:text-fg"
              }`}
            >
              Completed
            </button>
            <button
              type="button"
              onClick={() => setTab("archived")}
              className={`rounded px-2.5 py-0.5 transition-colors ${
                tab === "archived"
                  ? "bg-accent/20 text-accent font-semibold"
                  : "text-fg-soft hover:text-fg"
              }`}
            >
              Archived
            </button>
            <button
              type="button"
              onClick={() => setTab("clients")}
              className={`rounded px-2.5 py-0.5 transition-colors ${
                tab === "clients"
                  ? "bg-accent/20 text-accent font-semibold"
                  : "text-fg-soft hover:text-fg"
              }`}
            >
              Clients ({clients.length})
            </button>
          </div>
        </div>

        <div>
          {tab === "clients" ? (
            <button
              type="button"
              onClick={() => setShowNewClient(true)}
              className="rounded-md bg-accent px-3 py-1 text-[12px] font-semibold text-accent-fg hover:opacity-90 transition-opacity"
            >
              + New client
            </button>
          ) : (
            <button
              type="button"
              onClick={() => setShowNewProject(true)}
              className="rounded-md bg-accent px-3 py-1 text-[12px] font-semibold text-accent-fg hover:opacity-90 transition-opacity"
            >
              + New project
            </button>
          )}
        </div>
      </header>

      <div className="flex-1 overflow-y-auto p-5">
        {tab !== "clients" ? (
          <div className="rounded-lg border border-line bg-panel overflow-hidden">
            <table className="w-full text-left text-[12.5px]">
              <thead className="border-b border-line bg-surface text-[11px] font-semibold text-fg-faint uppercase">
                <tr>
                  <th className="px-4 py-2.5">Project</th>
                  <th className="px-4 py-2.5">Client</th>
                  <th className="px-4 py-2.5">Budget</th>
                  <th className="px-4 py-2.5">Rate</th>
                  <th className="px-4 py-2.5 text-right">Status</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-line-soft">
                {filteredProjects.map((p) => (
                  <tr key={p.id} className="hover:bg-surface transition-colors">
                    <td className="px-4 py-3">
                      <div className="flex items-center gap-2 font-medium text-fg-strong">
                        <span
                          className="size-2.5 rounded-full shrink-0"
                          style={{ backgroundColor: p.color }}
                        />
                        <span>{p.name}</span>
                      </div>
                      {p.description && (
                        <div className="text-[11px] text-fg-faint pl-4.5 mt-0.5">
                          {p.description}
                        </div>
                      )}
                    </td>
                    <td className="px-4 py-3 text-fg-soft">
                      {p.clientId ? clientMap.get(p.clientId) || "—" : "—"}
                    </td>
                    <td className="px-4 py-3 text-fg-soft font-mono">
                      {p.budgetKind !== "none" && p.budgetValue
                        ? `${p.budgetValue} ${p.budgetKind}`
                        : "no budget"}
                    </td>
                    <td className="px-4 py-3 text-fg-soft font-mono">
                      {p.hourlyRate ? `$${p.hourlyRate}/h` : "—"}
                    </td>
                    <td className="px-4 py-3 text-right">
                      <span className="rounded-sm bg-surface px-2 py-0.5 text-[11px] font-medium text-fg-muted uppercase">
                        {p.status}
                      </span>
                    </td>
                  </tr>
                ))}
                {filteredProjects.length === 0 && (
                  <tr>
                    <td
                      colSpan={5}
                      className="px-4 py-8 text-center text-fg-faint"
                    >
                      No {tab} projects found. Click "+ New project" to create
                      one.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        ) : (
          <div className="rounded-lg border border-line bg-panel overflow-hidden">
            <table className="w-full text-left text-[12.5px]">
              <thead className="border-b border-line bg-surface text-[11px] font-semibold text-fg-faint uppercase">
                <tr>
                  <th className="px-4 py-2.5">Client name</th>
                  <th className="px-4 py-2.5">Email</th>
                  <th className="px-4 py-2.5">Default rate</th>
                  <th className="px-4 py-2.5">Projects count</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-line-soft">
                {clients.map((c) => {
                  const clientProjs = projects.filter(
                    (p) => p.clientId === c.id,
                  );
                  return (
                    <tr
                      key={c.id}
                      className="hover:bg-surface transition-colors"
                    >
                      <td className="px-4 py-3 font-medium text-fg-strong">
                        {c.name}
                      </td>
                      <td className="px-4 py-3 text-fg-soft">
                        {c.email || "—"}
                      </td>
                      <td className="px-4 py-3 text-fg-soft font-mono">
                        {c.defaultRate ? `$${c.defaultRate}/h` : "—"}
                      </td>
                      <td className="px-4 py-3 text-fg-muted font-mono">
                        {clientProjs.length}
                      </td>
                    </tr>
                  );
                })}
                {clients.length === 0 && (
                  <tr>
                    <td
                      colSpan={4}
                      className="px-4 py-8 text-center text-fg-faint"
                    >
                      No clients created yet. Click "+ New client" to add one.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {/* New Project Modal */}
      {showNewProject && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-scrim/80 p-4">
          <form
            onSubmit={handleCreateProject}
            className="w-full max-w-md rounded-xl border border-line bg-panel p-5 shadow-2xl space-y-4"
          >
            <div className="flex items-center justify-between border-b border-line pb-2.5">
              <h2 className="text-[14px] font-semibold text-fg-strong">
                New project
              </h2>
              <button
                type="button"
                onClick={() => setShowNewProject(false)}
                className="text-fg-faint hover:text-fg"
              >
                ✕
              </button>
            </div>

            <div className="space-y-3 text-[12px]">
              <div>
                <label
                  htmlFor="new-proj-name"
                  className="block text-fg-soft mb-1 font-medium"
                >
                  Project name *
                </label>
                <input
                  id="new-proj-name"
                  type="text"
                  required
                  value={newProjName}
                  onChange={(e) => setNewProjName(e.target.value)}
                  placeholder="e.g. OpenRize Web"
                  className="w-full rounded-md border border-line bg-surface px-2.5 py-1.5 text-fg outline-hidden focus:border-accent"
                />
              </div>

              <div>
                <label
                  htmlFor="new-proj-color"
                  className="block text-fg-soft mb-1 font-medium"
                >
                  Color
                </label>
                <div className="flex items-center gap-2">
                  <input
                    id="new-proj-color"
                    type="color"
                    value={newProjColor}
                    onChange={(e) => setNewProjColor(e.target.value)}
                    className="size-8 cursor-pointer rounded border border-line bg-transparent"
                  />
                  <span className="font-mono text-fg-muted">
                    {newProjColor}
                  </span>
                </div>
              </div>

              <div>
                <label
                  htmlFor="new-proj-client"
                  className="block text-fg-soft mb-1 font-medium"
                >
                  Client (optional)
                </label>
                <select
                  id="new-proj-client"
                  value={newProjClientId}
                  onChange={(e) => setNewProjClientId(e.target.value)}
                  className="w-full rounded-md border border-line bg-surface px-2.5 py-1.5 text-fg outline-hidden focus:border-accent"
                >
                  <option value="">No Client</option>
                  {clients.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.name}
                    </option>
                  ))}
                </select>
              </div>

              <div>
                <label
                  htmlFor="new-proj-hints"
                  className="block text-fg-soft mb-1 font-medium"
                >
                  AI hints (folders, repos, domains)
                </label>
                <input
                  id="new-proj-hints"
                  type="text"
                  value={newProjHints}
                  onChange={(e) => setNewProjHints(e.target.value)}
                  placeholder="e.g. ~/Code/OpenRize, github.com/.../OpenRize"
                  className="w-full rounded-md border border-line bg-surface px-2.5 py-1.5 text-fg outline-hidden focus:border-accent"
                />
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label
                    htmlFor="new-proj-rate"
                    className="block text-fg-soft mb-1 font-medium"
                  >
                    Hourly rate ($)
                  </label>
                  <input
                    id="new-proj-rate"
                    type="number"
                    value={newProjRate || ""}
                    onChange={(e) =>
                      setNewProjRate(
                        e.target.value ? Number(e.target.value) : undefined,
                      )
                    }
                    placeholder="120"
                    className="w-full rounded-md border border-line bg-surface px-2.5 py-1.5 text-fg outline-hidden focus:border-accent"
                  />
                </div>
                <div>
                  <label
                    htmlFor="new-proj-budget-kind"
                    className="block text-fg-soft mb-1 font-medium"
                  >
                    Budget kind
                  </label>
                  <select
                    id="new-proj-budget-kind"
                    value={newProjBudgetKind}
                    onChange={(e) => setNewProjBudgetKind(e.target.value)}
                    className="w-full rounded-md border border-line bg-surface px-2.5 py-1.5 text-fg outline-hidden focus:border-accent"
                  >
                    <option value="none">None</option>
                    <option value="hours">Hours</option>
                    <option value="amount">Amount ($)</option>
                  </select>
                </div>
              </div>
            </div>

            <div className="flex justify-end gap-2 pt-2 border-t border-line">
              <button
                type="button"
                onClick={() => setShowNewProject(false)}
                className="rounded-md border border-line px-3 py-1.5 text-[12px] text-fg-soft hover:bg-surface hover:text-fg"
              >
                Cancel
              </button>
              <button
                type="submit"
                className="rounded-md bg-accent px-3 py-1.5 text-[12px] font-semibold text-accent-fg hover:opacity-90"
              >
                Create project
              </button>
            </div>
          </form>
        </div>
      )}

      {/* New Client Modal */}
      {showNewClient && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-scrim/80 p-4">
          <form
            onSubmit={handleCreateClient}
            className="w-full max-w-sm rounded-xl border border-line bg-panel p-5 shadow-2xl space-y-4"
          >
            <div className="flex items-center justify-between border-b border-line pb-2.5">
              <h2 className="text-[14px] font-semibold text-fg-strong">
                New client
              </h2>
              <button
                type="button"
                onClick={() => setShowNewClient(false)}
                className="text-fg-faint hover:text-fg"
              >
                ✕
              </button>
            </div>

            <div className="space-y-3 text-[12px]">
              <div>
                <label
                  htmlFor="new-client-name"
                  className="block text-fg-soft mb-1 font-medium"
                >
                  Client name *
                </label>
                <input
                  id="new-client-name"
                  type="text"
                  required
                  value={newClientName}
                  onChange={(e) => setNewClientName(e.target.value)}
                  placeholder="e.g. Acme Corp"
                  className="w-full rounded-md border border-line bg-surface px-2.5 py-1.5 text-fg outline-hidden focus:border-accent"
                />
              </div>

              <div>
                <label
                  htmlFor="new-client-email"
                  className="block text-fg-soft mb-1 font-medium"
                >
                  Email (optional)
                </label>
                <input
                  id="new-client-email"
                  type="email"
                  value={newClientEmail}
                  onChange={(e) => setNewClientEmail(e.target.value)}
                  placeholder="billing@acme.com"
                  className="w-full rounded-md border border-line bg-surface px-2.5 py-1.5 text-fg outline-hidden focus:border-accent"
                />
              </div>

              <div>
                <label
                  htmlFor="new-client-rate"
                  className="block text-fg-soft mb-1 font-medium"
                >
                  Default rate ($)
                </label>
                <input
                  id="new-client-rate"
                  type="number"
                  value={newClientRate || ""}
                  onChange={(e) =>
                    setNewClientRate(
                      e.target.value ? Number(e.target.value) : undefined,
                    )
                  }
                  placeholder="120"
                  className="w-full rounded-md border border-line bg-surface px-2.5 py-1.5 text-fg outline-hidden focus:border-accent"
                />
              </div>
            </div>

            <div className="flex justify-end gap-2 pt-2 border-t border-line">
              <button
                type="button"
                onClick={() => setShowNewClient(false)}
                className="rounded-md border border-line px-3 py-1.5 text-[12px] text-fg-soft hover:bg-surface hover:text-fg"
              >
                Cancel
              </button>
              <button
                type="submit"
                className="rounded-md bg-accent px-3 py-1.5 text-[12px] font-semibold text-accent-fg hover:opacity-90"
              >
                Create client
              </button>
            </div>
          </form>
        </div>
      )}
    </div>
  );
}
