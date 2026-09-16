import { useEffect, useState } from "react";
import { Sidebar, type SidebarView } from "./components/Sidebar";
import { Settings } from "./pages/Settings";
import { Trackers } from "./pages/Trackers";
import { useTimers } from "./hooks/useTimers";

export default function App() {
  const api = useTimers();
  const [view, setView] = useState<SidebarView>("trackers");

  /* ⌘, — the macOS Settings convention. No native app menu is built (see
     src-tauri/src/lib.rs), so nothing swallows the keystroke before the
     webview sees it. */
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (!event.metaKey || event.ctrlKey || event.altKey) return;
      if (event.key === ",") {
        event.preventDefault();
        setView("settings");
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  return (
    <div className="grid h-full min-h-0 grid-cols-[224px_minmax(0,1fr)] overflow-hidden">
      <Sidebar view={view} onSelect={setView} />
      <div className="flex min-h-0 min-w-0 flex-col">
        {view === "trackers" ? <Trackers api={api} /> : <Settings />}
      </div>
    </div>
  );
}
