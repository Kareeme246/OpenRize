import { useCallback, useEffect, useMemo, useState } from "react";
import { Picker } from "../components/Picker";
import * as api from "../lib/api";
import type { AppRecord, Category, Project } from "../lib/types";

export function Apps() {
  const [apps, setApps] = useState<AppRecord[]>([]);
  const [categories, setCategories] = useState<Category[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [search, setSearch] = useState("");
  const [filterKind, setFilterKind] = useState<"all" | "app" | "site">("all");
  const [filterExcluded, setFilterExcluded] = useState<
    "all" | "active" | "excluded"
  >("all");
  const [isLoading, setIsLoading] = useState(true);

  const loadData = useCallback(async () => {
    try {
      setIsLoading(true);
      const [allApps, cats, projs] = await Promise.all([
        api.listApps(),
        api.listCategories(),
        api.listProjects(),
      ]);
      setApps(allApps);
      setCategories(cats);
      setProjects(projs);
    } catch (err) {
      console.error("Failed to load apps data", err);
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    loadData();
  }, [loadData]);

  const handleUpdate = async (
    app: AppRecord,
    changes: {
      defaultCategoryId?: string;
      defaultProjectId?: string;
      excluded?: boolean;
    },
  ) => {
    try {
      const updated = await api.updateApp(
        app.id,
        changes.defaultCategoryId !== undefined
          ? changes.defaultCategoryId
          : app.defaultCategoryId,
        changes.defaultProjectId !== undefined
          ? changes.defaultProjectId
          : app.defaultProjectId,
        changes.excluded !== undefined ? changes.excluded : app.excluded,
      );
      setApps((prev) => prev.map((a) => (a.id === updated.id ? updated : a)));
    } catch (err) {
      console.error("Failed to update app", err);
    }
  };

  const filteredApps = useMemo(() => {
    const q = search.trim().toLowerCase();
    return apps.filter((app) => {
      if (filterKind !== "all" && app.kind !== filterKind) return false;
      if (filterExcluded === "active" && app.excluded) return false;
      if (filterExcluded === "excluded" && !app.excluded) return false;
      if (q) {
        return (
          app.displayName.toLowerCase().includes(q) ||
          app.identifier.toLowerCase().includes(q)
        );
      }
      return true;
    });
  }, [apps, search, filterKind, filterExcluded]);

  const activeCount = apps.filter((a) => !a.excluded).length;
  const excludedCount = apps.filter((a) => a.excluded).length;

  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto bg-base p-6">
      {/* Header */}
      <div className="mb-6 flex flex-wrap items-center justify-between gap-4">
        <div>
          <h1 className="text-xl font-bold tracking-tight text-fg">
            Apps & Websites
          </h1>
          <p className="text-xs text-fg-muted mt-0.5">
            Manage detected applications, map default categories and projects,
            or exclude sensitive tools from tracking.
          </p>
        </div>
        <div className="flex items-center gap-2 text-xs">
          <span className="rounded-md border border-line bg-surface px-2.5 py-1 font-medium text-fg-soft">
            Total: <span className="font-semibold text-fg">{apps.length}</span>
          </span>
          <span className="rounded-md border border-line bg-surface px-2.5 py-1 font-medium text-fg-soft">
            Tracking:{" "}
            <span className="font-semibold text-accent">{activeCount}</span>
          </span>
          {excludedCount > 0 && (
            <span className="rounded-md border border-line bg-surface px-2.5 py-1 font-medium text-fg-soft">
              Excluded:{" "}
              <span className="font-semibold text-review">{excludedCount}</span>
            </span>
          )}
        </div>
      </div>

      {/* Filter and Search Bar */}
      <div className="mb-4 flex flex-wrap items-center gap-3">
        <div className="relative min-w-[240px] flex-1 max-w-md">
          <input
            type="text"
            placeholder="Search by app name or bundle ID..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="w-full rounded-lg border border-line bg-surface px-3 py-1.5 pl-8 text-xs text-fg placeholder:text-fg-faint focus:border-accent focus:outline-none"
          />
          <svg
            className="absolute left-2.5 top-2 size-3.5 text-fg-faint"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth={2}
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <circle cx="11" cy="11" r="8" />
            <path d="m21 21-4.3-4.3" />
          </svg>
        </div>

        {/* Kind filter */}
        <div className="flex rounded-lg border border-line bg-surface p-0.5 text-xs">
          <button
            type="button"
            onClick={() => setFilterKind("all")}
            className={`rounded-md px-2.5 py-1 font-medium transition-colors ${
              filterKind === "all"
                ? "bg-accent/15 text-accent font-semibold"
                : "text-fg-soft hover:text-fg"
            }`}
          >
            All Types
          </button>
          <button
            type="button"
            onClick={() => setFilterKind("app")}
            className={`rounded-md px-2.5 py-1 font-medium transition-colors ${
              filterKind === "app"
                ? "bg-accent/15 text-accent font-semibold"
                : "text-fg-soft hover:text-fg"
            }`}
          >
            Apps
          </button>
          <button
            type="button"
            onClick={() => setFilterKind("site")}
            className={`rounded-md px-2.5 py-1 font-medium transition-colors ${
              filterKind === "site"
                ? "bg-accent/15 text-accent font-semibold"
                : "text-fg-soft hover:text-fg"
            }`}
          >
            Websites
          </button>
        </div>

        {/* Exclusion filter */}
        <div className="flex rounded-lg border border-line bg-surface p-0.5 text-xs">
          <button
            type="button"
            onClick={() => setFilterExcluded("all")}
            className={`rounded-md px-2.5 py-1 font-medium transition-colors ${
              filterExcluded === "all"
                ? "bg-accent/15 text-accent font-semibold"
                : "text-fg-soft hover:text-fg"
            }`}
          >
            All Status
          </button>
          <button
            type="button"
            onClick={() => setFilterExcluded("active")}
            className={`rounded-md px-2.5 py-1 font-medium transition-colors ${
              filterExcluded === "active"
                ? "bg-accent/15 text-accent font-semibold"
                : "text-fg-soft hover:text-fg"
            }`}
          >
            Tracked
          </button>
          <button
            type="button"
            onClick={() => setFilterExcluded("excluded")}
            className={`rounded-md px-2.5 py-1 font-medium transition-colors ${
              filterExcluded === "excluded"
                ? "bg-accent/15 text-accent font-semibold"
                : "text-fg-soft hover:text-fg"
            }`}
          >
            Excluded
          </button>
        </div>
      </div>

      {/* Table */}
      <div className="flex-1 overflow-hidden rounded-xl border border-line bg-surface">
        <div className="overflow-x-auto">
          <table className="w-full border-collapse text-left text-xs">
            <thead>
              <tr className="border-b border-line bg-surface-strong/60 text-fg-muted font-medium">
                <th className="py-2.5 pl-4 pr-3">Application / Website</th>
                <th className="px-3 py-2.5">Identifier</th>
                <th className="px-3 py-2.5">Default Category</th>
                <th className="px-3 py-2.5">Default Project</th>
                <th className="px-3 py-2.5 text-center">Tracking</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-line">
              {isLoading ? (
                <tr>
                  <td colSpan={5} className="py-8 text-center text-fg-muted">
                    Loading applications...
                  </td>
                </tr>
              ) : filteredApps.length === 0 ? (
                <tr>
                  <td colSpan={5} className="py-8 text-center text-fg-muted">
                    {search
                      ? "No applications matching query"
                      : "No applications recorded yet."}
                  </td>
                </tr>
              ) : (
                filteredApps.map((app) => (
                  <tr
                    key={app.id}
                    className={`transition-colors hover:bg-surface-strong/40 ${
                      app.excluded ? "opacity-60" : ""
                    }`}
                  >
                    <td className="py-2.5 pl-4 pr-3">
                      <div className="flex items-center gap-2.5">
                        <div className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-surface-strong text-xs font-bold text-fg-soft">
                          {app.displayName
                            ? app.displayName.charAt(0).toUpperCase()
                            : "?"}
                        </div>
                        <div className="min-w-0">
                          <div className="truncate font-semibold text-fg">
                            {app.displayName || app.identifier}
                          </div>
                          <span className="inline-block rounded bg-surface px-1.5 py-0.2 text-[10px] uppercase font-medium text-fg-faint">
                            {app.kind}
                          </span>
                        </div>
                      </div>
                    </td>

                    <td
                      className="px-3 py-2.5 text-fg-muted font-mono text-[11px] truncate max-w-[200px]"
                      title={app.identifier}
                    >
                      {app.identifier}
                    </td>

                    <td className="px-3 py-2.5">
                      <Picker
                        ariaLabel="Default category"
                        value={app.defaultCategoryId || ""}
                        disabled={app.excluded}
                        onChange={(val) =>
                          handleUpdate(app, {
                            defaultCategoryId: val || undefined,
                          })
                        }
                        options={[
                          { value: "", label: "(None)" },
                          ...categories.map((c) => ({
                            value: c.id,
                            label: c.name,
                            color: c.color,
                          })),
                        ]}
                        variant="compact"
                      />
                    </td>

                    <td className="px-3 py-2.5">
                      <Picker
                        ariaLabel="Default project"
                        value={app.defaultProjectId || ""}
                        disabled={app.excluded}
                        onChange={(val) =>
                          handleUpdate(app, {
                            defaultProjectId: val || undefined,
                          })
                        }
                        options={[
                          { value: "", label: "(None)" },
                          ...projects.map((p) => ({
                            value: p.id,
                            label: p.name,
                            color: p.color,
                          })),
                        ]}
                        variant="compact"
                      />
                    </td>

                    <td className="px-3 py-2.5 text-center">
                      <button
                        type="button"
                        onClick={() =>
                          handleUpdate(app, {
                            excluded: !app.excluded,
                          })
                        }
                        className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px] font-semibold transition-colors ${
                          app.excluded
                            ? "bg-review/20 text-review hover:bg-review/30"
                            : "bg-accent/15 text-accent hover:bg-accent/25"
                        }`}
                      >
                        {app.excluded ? "Excluded" : "Tracked"}
                      </button>
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
