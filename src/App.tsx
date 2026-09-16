import { useEffect, useState, type ReactElement } from "react";
import { Sidebar, type SidebarView } from "./components/Sidebar";
import { AiAgent } from "./pages/AiAgent";
import { Breaks } from "./pages/Breaks";
import { Categorization } from "./pages/Categorization";
import { DistractionBlocker } from "./pages/DistractionBlocker";
import { Focus } from "./pages/Focus";
import { Home } from "./pages/Home";
import { Integrations } from "./pages/Integrations";
import { Meetings } from "./pages/Meetings";
import { Reports } from "./pages/Reports";
import { Sessions } from "./pages/Sessions";
import { Settings } from "./pages/Settings";
import { Trackers } from "./pages/Trackers";
import { useTimers } from "./hooks/useTimers";

export default function App() {
  const api = useTimers();
  const [view, setView] = useState<SidebarView>("home");

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

  const renderView = (): ReactElement => {
    switch (view) {
      case "home":
        return <Home />;
      case "sessions":
        return <Sessions />;
      case "focus":
        return <Focus />;
      case "meetings":
        return <Meetings />;
      case "breaks":
        return <Breaks />;
      case "categorization":
        return <Categorization />;
      case "distraction-blocker":
        return <DistractionBlocker />;
      case "reports":
        return <Reports />;
      case "ai-agent":
        return <AiAgent />;
      case "integrations":
        return <Integrations />;
      case "trackers":
        return <Trackers api={api} />;
      case "settings":
        return <Settings />;
    }
  };

  return (
    <div className="grid h-full min-h-0 grid-cols-[224px_minmax(0,1fr)] overflow-hidden">
      <Sidebar view={view} onSelect={setView} />
      <div className="flex min-h-0 min-w-0 flex-col">{renderView()}</div>
    </div>
  );
}
