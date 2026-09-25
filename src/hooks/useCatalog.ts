import { useCallback, useEffect, useMemo, useState } from "react";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import type { Category, Client, Project } from "../lib/types";

export interface Catalog {
  categories: Category[];
  projects: Project[];
  clients: Client[];
  categoryById: Map<string, Category>;
  projectById: Map<string, Project>;
  clientById: Map<string, Client>;
  error: string | null;
  reload: () => Promise<void>;
}

/** Categories, projects, and clients: the names every entry view resolves. */
export function useCatalog(): Catalog {
  const [categories, setCategories] = useState<Category[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [clients, setClients] = useState<Client[]>([]);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async (): Promise<void> => {
    try {
      const [nextCategories, nextProjects, nextClients] = await Promise.all([
        api.listCategories(),
        api.listProjects(),
        api.listClients(),
      ]);
      setCategories(nextCategories);
      setProjects(nextProjects);
      setClients(nextClients);
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    }
  }, []);

  useEffect(() => {
    reload();
  }, [reload]);

  const categoryById = useMemo(
    () => new Map(categories.map((category) => [category.id, category])),
    [categories],
  );
  const projectById = useMemo(
    () => new Map(projects.map((project) => [project.id, project])),
    [projects],
  );
  const clientById = useMemo(
    () => new Map(clients.map((client) => [client.id, client])),
    [clients],
  );

  return {
    categories,
    projects,
    clients,
    categoryById,
    projectById,
    clientById,
    error,
    reload,
  };
}
