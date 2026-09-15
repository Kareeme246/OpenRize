import { formatDuration } from "../lib/timers";

interface AppHeaderProps {
  runningCount: number;
  totalMs: number;
}

export function AppHeader({ runningCount, totalMs }: AppHeaderProps) {
  return (
    <header className="flex shrink-0 items-center justify-between gap-4 border-b border-white/10 bg-black/25 px-5.5 py-3.5">
      <div className="min-w-0">
        <h1 className="text-[15px] font-semibold">Trackers</h1>
        <div className="font-mono text-[10.5px] text-white/35">manual stopwatches · saved as you click</div>
      </div>
      <div className="flex shrink-0 items-center gap-4">
        <div className="text-right">
          <b className="block font-mono text-[15px] font-semibold text-accent">{runningCount}</b>
          <span className="font-mono text-[9.5px] uppercase tracking-wider text-white/35">running</span>
        </div>
        <div className="text-right">
          <b className="block font-mono text-[15px] font-semibold text-accent">{formatDuration(totalMs)}</b>
          <span className="font-mono text-[9.5px] uppercase tracking-wider text-white/35">total</span>
        </div>
      </div>
    </header>
  );
}
