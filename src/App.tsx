import { AppHeader } from "./components/AppHeader";
import { Sidebar } from "./components/Sidebar";
import { Trackers } from "./pages/Trackers";

export default function App() {
  return (
    <div className="grid h-full grid-cols-[224px_minmax(0,1fr)]">
      <Sidebar />
      <div className="flex min-w-0 flex-col">
        <AppHeader runningCount={0} totalMs={0} />
        <Trackers />
      </div>
    </div>
  );
}
