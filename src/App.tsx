import { AppHeader } from "./components/AppHeader";
import { Sidebar } from "./components/Sidebar";
import { Trackers } from "./pages/Trackers";
import { elapsedMs } from "./lib/timers";
import { useTimers } from "./hooks/useTimers";

export default function App() {
  const api = useTimers();

  const runningCount = api.timers.filter(
    (timer) => timer.startedAt !== null,
  ).length;
  const totalMs = api.timers.reduce(
    (sum, timer) => sum + elapsedMs(timer, api.now),
    0,
  );

  return (
    <div className="grid h-full grid-cols-[224px_minmax(0,1fr)]">
      <Sidebar />
      <div className="flex min-w-0 flex-col">
        <AppHeader runningCount={runningCount} totalMs={totalMs} />
        <Trackers api={api} />
      </div>
    </div>
  );
}
