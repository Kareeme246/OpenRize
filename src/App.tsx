import { type ReactElement, useCallback, useEffect, useState } from "react";
import { NotImplementedProvider } from "./components/NotImplemented";
import { Sidebar, type SidebarView } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { useTimers } from "./hooks/useTimers";
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

/** Browser-style history: a stack plus where the user is standing in it. */
interface NavState {
  entries: SidebarView[];
  cursor: number;
}

export default function App() {
  const api = useTimers();
  const [nav, setNav] = useState<NavState>({ entries: ["home"], cursor: 0 });
  const view = nav.entries[nav.cursor];

  const select = useCallback((next: SidebarView): void => {
    setNav((previous) => {
      if (previous.entries[previous.cursor] === next) return previous;
      // Selecting from the middle of history discards the forward branch, the
      // same way a browser does.
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

  /* ⌘, — the macOS Settings convention. No native app menu is built (see
     src-tauri/src/lib.rs), so nothing swallows the keystroke before the
     webview sees it. */
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (!event.metaKey || event.ctrlKey || event.altKey) return;
      if (event.key === ",") {
        event.preventDefault();
        select("settings");
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [select]);

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
    <NotImplementedProvider>
      <div className="flex h-full min-h-0 flex-col overflow-hidden">
        <TopBar
          canGoBack={nav.cursor > 0}
          canGoForward={nav.cursor < nav.entries.length - 1}
          onBack={back}
          onForward={forward}
        />
        <div className="grid min-h-0 flex-1 grid-cols-[224px_minmax(0,1fr)] overflow-hidden">
          <Sidebar view={view} onSelect={select} />
          <div className="flex min-h-0 min-w-0 flex-col">{renderView()}</div>
        </div>
      </div>
    </NotImplementedProvider>
  );
}
