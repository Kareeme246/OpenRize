import { listen } from "@tauri-apps/api/event";
import { type ReactElement, useCallback, useEffect, useState } from "react";
import { NotImplementedProvider } from "./components/NotImplemented";
import { Sidebar } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { SettingsProvider } from "./hooks/useSettings";
import * as api from "./lib/api";
import type { ActivitySnapshot, ActivityTick, Route } from "./lib/types";
import { Apps } from "./pages/Apps";
import { Calendar } from "./pages/Calendar";
import { Invoices } from "./pages/Invoices";
import { MyTimesheet } from "./pages/MyTimesheet";
import { Projects } from "./pages/Projects";
import { Settings } from "./pages/Settings";
import { TimeEntries } from "./pages/TimeEntries";
import { Timesheets } from "./pages/Timesheets";

/** Browser-style history: a stack plus where the user is standing in it. */
interface NavState {
  entries: Route[];
  cursor: number;
}

export default function App() {
  const [nav, setNav] = useState<NavState>({
    entries: [{ name: "calendar" }],
    cursor: 0,
  });
  const currentRoute = nav.entries[nav.cursor] || { name: "calendar" };

  const [captureEnabled, setCaptureEnabledState] = useState(true);
  const [currentApp, setCurrentApp] = useState<string | undefined>(undefined);
  const [pendingCount, setPendingCount] = useState(0);

  const navigate = useCallback((next: Route): void => {
    setNav((previous) => {
      const current = previous.entries[previous.cursor];
      if (current && JSON.stringify(current) === JSON.stringify(next)) {
        return previous;
      }
      const entries = [...previous.entries.slice(0, previous.cursor + 1), next];
      return { entries, cursor: entries.length - 1 };
    });
  }, []);

  const back = useCallback((): void => {
    setNav((previous) => ({
      ...previous,
      cursor: Math.max(0, previous.cursor - 1),
    }));
  }, []);

  const forward = useCallback((): void => {
    setNav((previous) => ({
      ...previous,
      cursor: Math.min(previous.entries.length - 1, previous.cursor + 1),
    }));
  }, []);

  /* ⌘, — the macOS Settings convention. */
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (!event.metaKey || event.ctrlKey || event.altKey) return;
      if (event.key === ",") {
        event.preventDefault();
        navigate({ name: "settings" });
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [navigate]);

  // Initial load & capture events
  useEffect(() => {
    // Load initial snapshot
    api
      .fetchActivitySnapshot(0)
      .then((snapshot) => {
        setCaptureEnabledState(snapshot.captureEnabled);
        if (snapshot.current) {
          setCurrentApp(snapshot.current.app);
        }
      })
      .catch((err) => {
        console.error("Failed to load activity snapshot", err);
      });

    // Listen to tick events
    const unlistenTick = listen<ActivityTick>(api.ACTIVITY_TICK, (event) => {
      setCaptureEnabledState(event.payload.captureEnabled);
      if (event.payload.current) {
        setCurrentApp(event.payload.current.app);
      }
    });

    // Listen to activity changed events
    const unlistenActivity = listen<ActivitySnapshot>(
      api.ACTIVITY_CHANGED,
      (event) => {
        setCaptureEnabledState(event.payload.captureEnabled);
        if (event.payload.current) {
          setCurrentApp(event.payload.current.app);
        }
      },
    );

    return () => {
      void unlistenTick.then((u) => u());
      void unlistenActivity.then((u) => u());
    };
  }, []);

  // Entries waiting for review today (the Calendar's pending badge).
  useEffect(() => {
    const updatePending = async () => {
      try {
        const startOfDay = new Date();
        startOfDay.setHours(0, 0, 0, 0);
        const entries = await api.listTimeEntries(
          startOfDay.getTime(),
          Date.now(),
        );
        const count = entries.filter((e) => e.status === "pending").length;
        setPendingCount(count);
      } catch (err) {
        console.error("Failed to load pending entries count", err);
      }
    };

    updatePending();
    const unlistenEntries = listen(api.ENTRIES_CHANGED, updatePending);
    const unlistenSuggestion = listen(api.SUGGESTION_READY, updatePending);
    return () => {
      void unlistenEntries.then((u) => u());
      void unlistenSuggestion.then((u) => u());
    };
  }, []);

  const handleToggleCapture = async () => {
    try {
      const next = !captureEnabled;
      await api.setCaptureEnabled(next);
      setCaptureEnabledState(next);
    } catch (err) {
      console.error("Failed to toggle capture", err);
    }
  };

  const renderView = (): ReactElement => {
    switch (currentRoute.name) {
      case "calendar":
        return <Calendar />;
      case "timesheet":
        return <MyTimesheet />;
      case "apps":
        return <Apps />;
      case "entries":
        return <TimeEntries />;
      case "timesheets":
        return <Timesheets />;
      case "projects":
        return <Projects />;
      case "invoices":
        return <Invoices />;
      case "settings":
        return <Settings />;
    }
  };

  return (
    <SettingsProvider>
      <NotImplementedProvider>
        <div className="flex h-full min-h-0 flex-col overflow-hidden">
          <TopBar
            canGoBack={nav.cursor > 0}
            canGoForward={nav.cursor < nav.entries.length - 1}
            onBack={back}
            onForward={forward}
          />
          <div className="grid min-h-0 flex-1 grid-cols-[224px_minmax(0,1fr)] overflow-hidden">
            <Sidebar
              currentRoute={currentRoute}
              onNavigate={navigate}
              pendingCount={pendingCount}
              currentApp={currentApp}
              captureEnabled={captureEnabled}
              onToggleCapture={handleToggleCapture}
            />
            <div className="flex min-h-0 min-w-0 flex-col">{renderView()}</div>
          </div>
        </div>
      </NotImplementedProvider>
    </SettingsProvider>
  );
}
