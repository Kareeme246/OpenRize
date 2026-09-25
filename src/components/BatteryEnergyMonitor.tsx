import { useState } from "react";
import type { EnergyImpactLevel, EnergySummary } from "../lib/types";
import { ConfirmDialog } from "./ConfirmDialog";
import { SegmentedControl, type SegmentedOption } from "./SegmentedControl";

const IMPACT_STYLES: Record<
  EnergyImpactLevel,
  { badge: string; dot: string; label: string }
> = {
  low: {
    badge: "border-emerald-500/40 bg-emerald-500/10 text-emerald-500",
    dot: "bg-emerald-500",
    label: "Low",
  },
  medium: {
    badge: "border-amber-500/40 bg-amber-500/10 text-amber-500",
    dot: "bg-amber-500",
    label: "Medium",
  },
  high: {
    badge: "border-rose-500/40 bg-rose-500/10 text-rose-500",
    dot: "bg-rose-500",
    label: "High",
  },
};

const RANGE_OPTIONS: SegmentedOption<number>[] = [
  { value: 1, label: "24h" },
  { value: 7, label: "7d" },
  { value: 30, label: "30d" },
];

function StatCard({
  label,
  value,
  detail,
}: {
  label: string;
  value: string;
  detail: string;
}) {
  return (
    <div className="min-w-0 rounded-lg border border-line-soft bg-inset-soft px-3 py-2">
      <div className="font-mono text-[10px] uppercase tracking-wider text-fg-faint">
        {label}
      </div>
      <div className="mt-0.5 text-[17px] font-semibold tabular-nums text-fg-strong">
        {value}
      </div>
      <div className="truncate text-[10.5px] text-fg-faint" title={detail}>
        {detail}
      </div>
    </div>
  );
}

interface BatteryEnergyMonitorProps {
  summary: EnergySummary | null;
  loading: boolean;
  selectedDays: number;
  onRangeChange: (days: number) => void;
  onReset: () => Promise<void>;
}

export function BatteryEnergyMonitor({
  summary,
  loading,
  selectedDays,
  onRangeChange,
  onReset,
}: BatteryEnergyMonitorProps) {
  const [confirmReset, setConfirmReset] = useState(false);
  const [resetting, setResetting] = useState(false);

  if (summary === null && loading) {
    return (
      <div className="p-4 text-[11.5px] text-fg-faint">
        Loading energy & battery telemetry…
      </div>
    );
  }

  if (summary === null) {
    return (
      <div className="p-4 text-[11.5px] text-fg-faint">
        No energy telemetry recorded yet.
      </div>
    );
  }

  const impact = IMPACT_STYLES[summary.currentImpact] ?? IMPACT_STYLES.low;
  const aiPct = summary.aiEnergyPct;
  const ambientPct = Math.max(0, 100 - aiPct);

  const handleConfirmReset = async () => {
    try {
      setResetting(true);
      await onReset();
    } finally {
      setResetting(false);
      setConfirmReset(false);
    }
  };

  return (
    <div className="flex flex-col">
      {/* Current Impact Row */}
      <div className="flex items-center justify-between gap-4 border-b border-line px-4 py-3">
        <div className="min-w-0">
          <div className="text-[13px] font-medium text-fg">
            Current energy impact
          </div>
          <div className="text-[11.5px] text-fg-faint">
            {summary.impactDescription}
          </div>
        </div>
        <div className="flex items-center gap-2">
          <span
            className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 font-mono text-[10px] font-medium uppercase tracking-wider ${impact.badge}`}
          >
            <span className={`size-1.5 rounded-full ${impact.dot}`} />
            {impact.label}
          </span>
          <span className="font-mono text-[11px] tabular-nums text-fg-muted">
            {summary.currentPowerWatts.toFixed(2)} W
          </span>
        </div>
      </div>

      {/* Battery Used Row */}
      <div className="flex items-center justify-between gap-4 border-b border-line px-4 py-3">
        <div className="min-w-0">
          <div className="text-[13px] font-medium text-fg">
            Battery consumed
          </div>
          <div className="text-[11.5px] text-fg-faint">
            {summary.onBattery
              ? `Running on battery (${summary.currentBatteryPct !== null && summary.currentBatteryPct !== undefined ? `${Math.round(summary.currentBatteryPct)}%` : "discharging"})`
              : "Connected to power (AC)"}
            {" · "}
            Hardware energy telemetry from OpenRize & AI sidecar
          </div>
        </div>
        <div className="text-right">
          <div className="font-mono text-[13px] font-semibold tabular-nums text-fg-strong">
            ~{summary.batteryUsedPct.toFixed(2)}%
          </div>
          <div className="font-mono text-[10.5px] text-fg-faint">
            ~{summary.batteryUsedMwh.toFixed(1)} mWh
          </div>
        </div>
      </div>

      {/* Benchmarking & Telemetry Block */}
      <div className="px-4 py-3">
        <div className="flex items-center justify-between gap-4 pb-3">
          <div>
            <div className="text-[13px] font-medium text-fg">
              Energy telemetry & benchmarks
            </div>
            <div className="text-[11.5px] text-fg-faint">
              Measured over the last{" "}
              {selectedDays === 1 ? "24 hours" : `${selectedDays} days`}
            </div>
          </div>
          <SegmentedControl
            name="energy-benchmark-range"
            value={selectedDays}
            options={RANGE_OPTIONS}
            onChange={onRangeChange}
          />
        </div>

        {/* Energy Share Stacked Bar */}
        <div className="my-2">
          <div className="flex h-2 overflow-hidden rounded-[3px] bg-inset-soft">
            <div
              className="bg-accent transition-all duration-300"
              style={{ width: `${aiPct}%` }}
              title={`On-device AI classification: ${aiPct.toFixed(1)}%`}
            />
            <div
              className="bg-line-strong transition-all duration-300"
              style={{ width: `${ambientPct}%` }}
              title={`Ambient tracking & UI: ${ambientPct.toFixed(1)}%`}
            />
          </div>
          <div className="mt-1.5 flex items-center justify-between text-[11px] text-fg-faint">
            <span className="flex items-center gap-1.5">
              <span className="size-2 rounded-[2px] bg-accent" />
              On-device AI classification:{" "}
              <span className="font-mono text-fg">{aiPct.toFixed(1)}%</span>
            </span>
            <span className="flex items-center gap-1.5">
              <span className="size-2 rounded-[2px] bg-line-strong" />
              Ambient tracking:{" "}
              <span className="font-mono text-fg">
                {ambientPct.toFixed(1)}%
              </span>
            </span>
          </div>
        </div>

        {/* Stats Grid */}
        <div className="mt-3.5 grid grid-cols-2 gap-2.5 sm:grid-cols-4">
          <StatCard
            label="Baseline"
            value={`${summary.baselinePowerWatts.toFixed(2)} W`}
            detail={`Peak: ${summary.peakPowerWatts.toFixed(2)} W`}
          />
          <StatCard
            label="AI Energy Share"
            value={`${summary.aiEnergyPct.toFixed(1)}%`}
            detail="Classification & training"
          />
          <StatCard
            label="Total Energy"
            value={`${summary.totalEnergyJoules.toFixed(1)} J`}
            detail={`${(summary.totalCpuTimeMs / 1000).toFixed(1)}s CPU`}
          />
          <StatCard
            label="Samples"
            value={`${summary.samplesCount}`}
            detail="15s interval"
          />
        </div>

        {/* Benchmarking Actions */}
        <div className="mt-3 flex items-center justify-between pt-2">
          <span className="text-[11px] text-fg-faint">
            Telemetry persists in SQLite across restarts for repeatable
            benchmarks.
          </span>
          <button
            type="button"
            disabled={resetting || summary.samplesCount === 0}
            onClick={() => setConfirmReset(true)}
            className="rounded-lg border border-line bg-surface px-2.5 py-1 text-[11px] font-medium text-fg-muted hover:bg-surface-strong disabled:opacity-40"
          >
            Reset benchmark…
          </button>
        </div>
      </div>

      {confirmReset && (
        <ConfirmDialog
          title="Reset energy telemetry?"
          body="This clears recorded energy telemetry and resets the benchmark baseline. Your time entries, categories, and AI personal models are unaffected."
          confirmLabel="Reset telemetry"
          onConfirm={handleConfirmReset}
          onCancel={() => setConfirmReset(false)}
        />
      )}
    </div>
  );
}
