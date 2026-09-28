import { type ReactElement, useCallback, useEffect, useState } from "react";
import { NotImplementedProvider } from "./components/NotImplemented";
import { Sidebar } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { SettingsProvider } from "./hooks/useSettings";
import { useTauriEvent } from "./hooks/useTauriEvent";
import { useUpdates } from "./hooks/useUpdates";
import * as api from "./lib/api";
import { currentCalendarDay, localDateString } from "./lib/dates";
import type { ActivitySnapshot, ActivityTick, Route } from "./lib/types";
import { Apps } from "./pages/Apps";
import { Calendar } from "./pages/Calendar";
import { Invoices } from "./pages/Invoices";
import { MyTimesheet } from "./pages/MyTimesheet";
import { Projects } from "./pages/Projects";
import { Settings } from "./pages/Settings";
import { TimeEntries } from "./pages/TimeEntries";
import { Timers } from "./pages/Timers";
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
  const [trackingActive, setTrackingActive] = useState(true);
  const [currentApp, setCurrentApp] = useState<string | undefined>(undefined);
  const [pendingCount, setPendingCount] = useState(0);
  // Bumped to remount the Calendar, so a review request always starts fresh
  // on today, even when that exact route is already showing.
  const [calendarMount, setCalendarMount] = useState(0);
  // Bumped by the sidebar's "Update available", so Settings scrolls to its
  // Updates section even when Settings is already open.
  const [revealUpdates, setRevealUpdates] = useState(0);
  const updates = useUpdates();

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

  /** Updates the current route in place: filters and tabs, not new pages. */
  const replace = useCallback((next: Route): void => {
    setNav((previous) => {
      const entries = [...previous.entries];
      entries[previous.cursor] = next;
      return { ...previous, entries };
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
    api
      .fetchActivitySnapshot(0)
      .then((snapshot) => {
        setCaptureEnabledState(snapshot.captureEnabled);
        if (snapshot.trackingActive !== undefined) {
          setTrackingActive(snapshot.trackingActive);
        }
        if (snapshot.current) {
          setCurrentApp(snapshot.current.app);
        } else {
          setCurrentApp(undefined);
        }
      })
      .catch((err) => {
        console.error("Failed to load activity snapshot", err);
      });
  }, []);

  const adoptCapture = (payload: ActivityTick | ActivitySnapshot): void => {
    setCaptureEnabledState(payload.captureEnabled);
    if (payload.trackingActive !== undefined) {
      setTrackingActive(payload.trackingActive);
    }
    if (payload.current) {
      setCurrentApp(payload.current.app);
    } else {
      setCurrentApp(undefined);
    }
  };
  useTauriEvent<ActivityTick>(api.ACTIVITY_TICK, adoptCapture);
  useTauriEvent<ActivitySnapshot>(api.ACTIVITY_CHANGED, adoptCapture);

  // Entries waiting for review today (the Calendar's pending badge).
  const updatePending = useCallback(async (): Promise<void> => {
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
  }, []);
  useEffect(() => {
    void updatePending();
  }, [updatePending]);
  useTauriEvent(api.ENTRIES_CHANGED, updatePending);
  useTauriEvent(api.SUGGESTION_READY, updatePending);

  // The menu-bar panel's "N to review" button.
  useTauriEvent(api.OPEN_REVIEW, () => {
    navigate({
      name: "calendar",
      scale: "day",
      date: localDateString(currentCalendarDay(new Date())),
      review: true,
    });
    setCalendarMount((count) => count + 1);
  });

  const handleToggleCapture = async () => {
    try {
      const next = !trackingActive;
      await api.setCaptureEnabled(next);
      setTrackingActive(next);
      if (next) {
        setCaptureEnabledState(true);
      }
    } catch (err) {
      console.error("Failed to toggle capture", err);
    }
  };

  const renderView = (): ReactElement => {
    switch (currentRoute.name) {
      case "calendar":
        return (
          <Calendar
            key={calendarMount}
            route={currentRoute}
            navigate={navigate}
          />
        );
      case "timesheet":
        return (
          <MyTimesheet
            route={currentRoute}
            navigate={navigate}
            replace={replace}
          />
        );
      case "timers":
        return <Timers />;
      case "apps":
        return <Apps />;
      case "entries":
        return (
          <TimeEntries
            route={currentRoute}
            navigate={navigate}
            replace={replace}
          />
        );
      case "timesheets":
        return <Timesheets navigate={navigate} />;
      case "projects":
        return (
          <Projects
            route={currentRoute}
            navigate={navigate}
            replace={replace}
          />
        );
      case "invoices":
        return <Invoices />;
      case "settings":
        return (
          <Settings
            route={currentRoute}
            updates={updates}
            revealUpdates={revealUpdates}
          />
        );
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
              trackingActive={trackingActive}
              onToggleCapture={handleToggleCapture}
              updateVersion={updates?.available?.version}
              onOpenUpdate={() => {
                navigate({ name: "settings", section: "updates" });
                setRevealUpdates((count) => count + 1);
              }}
            />
            <div className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden">
              {renderView()}
            </div>
          </div>
        </div>
      </NotImplementedProvider>
    </SettingsProvider>
  );
}
