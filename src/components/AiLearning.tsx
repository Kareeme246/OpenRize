/**
 * Settings → Categories & AI, the learning-loop half: the auto-accept
 * threshold with its expected-error preview, AI effectiveness (rates, the
 * tier split, and the calibration chart), and the personal models.
 *
 * Everything here renders a snapshot Rust computed (`ai_metrics`); nothing is
 * derived from local state beyond formatting.
 */
import { useEffect, useId, useState } from "react";
import { percent } from "../lib/confidence";
import type {
  AiMetrics,
  AiStatus,
  ArtifactVersion,
  ReliabilityBin,
} from "../lib/types";

const THRESHOLD_MIN = 80;
const THRESHOLD_MAX = 99;
/** Model suggestions show at most this until calibration starts. */
const COLD_START_CAP_PERCENT = 90;
const CALIBRATION_MIN = 50;
const SAVE_DEBOUNCE_MS = 350;

function share(part: number, whole: number): string {
  return whole === 0 ? "-" : percent(part / whole);
}

function plural(count: number, one: string, many = `${one}s`): string {
  return `${count} ${count === 1 ? one : many}`;
}

function ago(epochMs: number, now: number = Date.now()): string {
  const minutes = Math.max(0, Math.round((now - epochMs) / 60_000));
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours}h ago`;
  return `${Math.round(hours / 24)}d ago`;
}

// --- Auto-accept threshold ----------------------------------------------------

/** "At 95%, 38% of the last 30 days' entries would have auto-approved…" */
function previewText(
  metrics: AiMetrics | null,
  value: number,
  calibrated: boolean,
): string {
  if (metrics === null) return "Loading the preview…";
  const period = `the last ${metrics.days} days`;
  if (metrics.entries === 0) {
    return `No suggested entries in ${period} to preview against yet.`;
  }
  const row = metrics.thresholds.find((t) => t.percent === value);
  if (row === undefined) return "";
  const wrong =
    row.autoApproved === 0
      ? ""
      : row.wrong === 0
        ? ", and none of them would have been wrong"
        : `, and ${row.wrong} of them would have been wrong`;
  const capped =
    !calibrated && value > COLD_START_CAP_PERCENT
      ? ` Until calibration starts, model suggestions stop at ${COLD_START_CAP_PERCENT}%, so only rule matches clear it.`
      : "";
  return `At ${value}%, ${share(row.autoApproved, metrics.entries)} of ${period}' entries (${row.autoApproved} of ${metrics.entries}) would have auto-approved${wrong}.${capped}`;
}

/**
 * A slider with a live readout. The preview follows the thumb; the setting
 * saves once dragging settles, so settings.json isn't rewritten per pixel.
 */
export function ThresholdSlider({
  value,
  disabled,
  metrics,
  calibrated,
  onChange,
}: {
  value: number;
  disabled: boolean;
  metrics: AiMetrics | null;
  calibrated: boolean;
  onChange: (value: number) => void;
}) {
  const [draft, setDraft] = useState(value);
  const id = useId();

  useEffect(() => setDraft(value), [value]);
  useEffect(() => {
    if (draft === value) return;
    const timer = setTimeout(() => onChange(draft), SAVE_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [draft, value, onChange]);

  // A value set outside the usual range (settings.json allows 50-100) widens
  // the track instead of being shown as something it isn't.
  const min = Math.min(THRESHOLD_MIN, value);
  const max = Math.max(THRESHOLD_MAX, value);
  const clamped = Math.min(max, Math.max(min, draft));
  const fill = ((clamped - min) / (max - min)) * 100;

  return (
    <div className={disabled ? "opacity-50" : undefined}>
      <div className="flex items-center gap-3">
        <input
          id={id}
          type="range"
          aria-label="Auto-accept threshold"
          aria-valuetext={`${clamped}%`}
          min={min}
          max={max}
          step={1}
          value={clamped}
          disabled={disabled}
          onChange={(event) => setDraft(Number(event.target.value))}
          style={{
            background: `linear-gradient(to right, var(--accent) ${fill}%, var(--bg-surface-2) ${fill}%)`,
          }}
          className="threshold-range h-1.5 min-w-0 flex-1 cursor-pointer appearance-none rounded-full disabled:cursor-not-allowed"
        />
        <label
          htmlFor={id}
          className="shrink-0 whitespace-nowrap text-right font-mono text-[12px] tabular-nums text-fg"
        >
          ≥ {clamped}%{" "}
          <span className="text-fg-faint">
            {calibrated ? "calibrated" : "capped"}
          </span>
        </label>
      </div>
      <p className="mt-2 text-[11.5px] leading-relaxed text-fg-soft">
        {previewText(metrics, clamped, calibrated)}
      </p>
    </div>
  );
}

// --- AI effectiveness ------------------------------------------------------------

function Stat({
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

const TIERS = [
  { key: "rule", label: "Rules", fill: "bg-tier-rule" },
  { key: "personal", label: "Personal", fill: "bg-tier-personal" },
  { key: "model", label: "Model", fill: "bg-tier-model" },
] as const;

/** Rules / Personal / Model share of decisions, as one stacked bar. */
function TierSplit({ tiers }: { tiers: AiMetrics["tiers"] }) {
  const [hover, setHover] = useState<string | null>(null);
  const total = tiers.rule + tiers.personal + tiers.model;
  if (total === 0) {
    return (
      <p className="text-[11.5px] text-fg-faint">
        No decisions in this period yet.
      </p>
    );
  }
  const hovered = TIERS.find((tier) => tier.key === hover);
  return (
    <div>
      <div className="flex h-2.5 gap-[2px]">
        {TIERS.filter((tier) => tiers[tier.key] > 0).map((tier) => (
          <div
            key={tier.key}
            role="img"
            aria-label={`${tier.label}: ${plural(tiers[tier.key], "decision")}`}
            onMouseEnter={() => setHover(tier.key)}
            onMouseLeave={() => setHover(null)}
            className={`${tier.fill} min-w-[4px] rounded-[3px] transition-opacity ${
              hover !== null && hover !== tier.key ? "opacity-50" : ""
            }`}
            style={{ flexGrow: tiers[tier.key] }}
          />
        ))}
      </div>
      <div className="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-[11.5px] text-fg-muted">
        {TIERS.map((tier) => (
          <span key={tier.key} className="flex items-center gap-1.5">
            <span className={`size-2 rounded-[2px] ${tier.fill}`} />
            {tier.label}{" "}
            <span className="tabular-nums text-fg">
              {share(tiers[tier.key], total)}
            </span>
          </span>
        ))}
        <span className="ml-auto font-mono text-[10.5px] text-fg-faint">
          {hovered === undefined
            ? `of ${plural(total, "decision")}`
            : `${hovered.label}: ${plural(tiers[hovered.key], "decision")}`}
        </span>
      </div>
    </div>
  );
}

const CHART = {
  width: 300,
  height: 190,
  left: 34,
  right: 10,
  top: 10,
  bottom: 28,
};

/**
 * Reliability diagram: each dot is a 10% bucket of displayed confidence,
 * placed at what was promised (x) vs how often it was accepted (y). On the
 * dashed diagonal, "90%" meant 9 in 10.
 */
function CalibrationChart({ bins }: { bins: ReliabilityBin[] }) {
  const [hover, setHover] = useState<number | null>(null);
  const plotW = CHART.width - CHART.left - CHART.right;
  const plotH = CHART.height - CHART.top - CHART.bottom;
  const x = (p: number): number => CHART.left + p * plotW;
  const y = (p: number): number => CHART.top + (1 - p) * plotH;
  const most = Math.max(1, ...bins.map((b) => b.count));
  const radius = (count: number): number => 4 + 5 * Math.sqrt(count / most);
  const ticks = [0, 0.25, 0.5, 0.75, 1];
  const active = hover === null ? null : bins[hover];

  return (
    <div className="relative">
      <svg
        viewBox={`0 0 ${CHART.width} ${CHART.height}`}
        className="h-auto w-full max-w-[420px]"
        role="img"
        aria-label="Calibration: displayed confidence against actual accept rate"
        onMouseLeave={() => setHover(null)}
      >
        {ticks.map((tick) => (
          <g key={tick}>
            <line
              x1={x(0)}
              x2={x(1)}
              y1={y(tick)}
              y2={y(tick)}
              stroke="var(--line)"
              strokeWidth={1}
            />
            <text
              x={x(0) - 6}
              y={y(tick) + 3}
              textAnchor="end"
              fontSize={9}
              fill="var(--fg-faint)"
            >
              {Math.round(tick * 100)}%
            </text>
            <text
              x={x(tick)}
              y={CHART.height - CHART.bottom + 13}
              textAnchor="middle"
              fontSize={9}
              fill="var(--fg-faint)"
            >
              {Math.round(tick * 100)}%
            </text>
          </g>
        ))}
        <line
          x1={x(0)}
          y1={y(0)}
          x2={x(1)}
          y2={y(1)}
          stroke="var(--fg-soft)"
          strokeWidth={1}
          strokeDasharray="3 3"
        />
        <text
          x={x(0.2)}
          y={y(0.08)}
          textAnchor="start"
          fontSize={9}
          fill="var(--fg-soft)"
        >
          perfect calibration
        </text>
        {bins.map((bin, index) => (
          // biome-ignore lint/a11y/noStaticElementInteractions: hover-only detail; the same numbers are in the table below
          <g
            key={bin.lo}
            onMouseEnter={() => setHover(index)}
            className="cursor-default"
          >
            <circle
              cx={x(bin.predicted)}
              cy={y(bin.actual)}
              r={12}
              fill="transparent"
            />
            <circle
              cx={x(bin.predicted)}
              cy={y(bin.actual)}
              r={radius(bin.count)}
              fill="var(--accent)"
              fillOpacity={hover === null || hover === index ? 0.9 : 0.35}
              stroke="var(--bg-panel)"
              strokeWidth={2}
            />
          </g>
        ))}
        <text
          x={x(0.5)}
          y={CHART.height - 2}
          textAnchor="middle"
          fontSize={9.5}
          fill="var(--fg-soft)"
        >
          Shown confidence
        </text>
        <text
          x={9}
          y={y(0.5)}
          textAnchor="middle"
          fontSize={9.5}
          fill="var(--fg-soft)"
          transform={`rotate(-90 9 ${y(0.5)})`}
        >
          Accepted
        </text>
      </svg>
      {active !== null && (
        <div
          className="pointer-events-none absolute rounded-md border border-line bg-panel px-2 py-1 text-[11px] text-fg shadow-lg"
          style={{
            left: `${(x(active.predicted) / CHART.width) * 100}%`,
            top: `${(y(active.actual) / CHART.height) * 100}%`,
            transform: "translate(-50%, calc(-100% - 12px))",
          }}
        >
          <div className="font-medium">
            Shown {percent(active.lo)}–{percent(active.hi)}
          </div>
          <div className="text-fg-muted">
            {plural(active.count, "suggestion")} · {percent(active.actual)}{" "}
            accepted
          </div>
        </div>
      )}
      <table className="sr-only">
        <caption>Calibration by shown confidence</caption>
        <thead>
          <tr>
            <th>Shown</th>
            <th>Suggestions</th>
            <th>Average shown</th>
            <th>Accepted</th>
          </tr>
        </thead>
        <tbody>
          {bins.map((bin) => (
            <tr key={bin.lo}>
              <td>
                {percent(bin.lo)}–{percent(bin.hi)}
              </td>
              <td>{bin.count}</td>
              <td>{percent(bin.predicted)}</td>
              <td>{percent(bin.actual)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function calibrationSummary(
  metrics: AiMetrics,
  status: AiStatus | null,
): string {
  const fitted = metrics.calibration.history.find((v) => v.active);
  if (status?.calibrated && fitted !== undefined) {
    return `Calibrated on your last ${plural(fitted.examples, "review")}, refit ${ago(fitted.trainedAt)}.`;
  }
  const outcomes = status?.outcomes ?? 0;
  return `Learning: ${Math.min(outcomes, CALIBRATION_MIN)} of ${CALIBRATION_MIN} reviews before calibration starts. Until then, confidence is capped at ${COLD_START_CAP_PERCENT}%.`;
}

export function AiEffectiveness({
  metrics,
  status,
}: {
  metrics: AiMetrics;
  status: AiStatus | null;
}) {
  const { bins, samples, expectedError } = metrics.calibration;
  return (
    <div className="flex flex-col gap-4">
      <div className="grid grid-cols-3 gap-2">
        <Stat
          label="Acceptance"
          value={share(metrics.accepted, metrics.reviewed)}
          detail={`${metrics.accepted} of ${plural(metrics.reviewed, "review")}`}
        />
        <Stat
          label="Auto-approved"
          value={share(metrics.autoApproved, metrics.decided)}
          detail={`${metrics.autoApproved} of ${metrics.decided} decided entries`}
        />
        <Stat
          label="Changed"
          value={share(metrics.changed, metrics.reviewed)}
          detail={`${metrics.changed} changed · ${metrics.rejected} rejected`}
        />
      </div>

      <div>
        <div className="mb-2 text-[12px] font-medium text-fg">Who decided</div>
        <TierSplit tiers={metrics.tiers} />
      </div>

      <div>
        <div className="flex items-baseline justify-between gap-3">
          <div className="text-[12px] font-medium text-fg">
            Calibration: shown vs accepted
          </div>
          {expectedError !== undefined && (
            <span className="shrink-0 font-mono text-[10.5px] text-fg-faint">
              avg gap {Math.round(expectedError * 100)} pts
            </span>
          )}
        </div>
        <p className="mb-2 text-[11.5px] text-fg-faint">
          {calibrationSummary(metrics, status)}
        </p>
        {samples === 0 ? (
          <p className="rounded-lg border border-dashed border-line px-3 py-4 text-center text-[11.5px] text-fg-faint">
            No reviewed model suggestions in the last {metrics.days} days yet.
            Rule matches are certain, so they aren't charted.
          </p>
        ) : (
          <div className="grid items-start gap-4 sm:grid-cols-[minmax(0,420px)_minmax(0,1fr)]">
            <CalibrationChart bins={bins} />
            <ul className="flex flex-col gap-2 text-[11.5px] leading-relaxed text-fg-faint">
              <li>
                Each dot groups suggestions shown at a similar confidence. Its
                height is how many of them you accepted; bigger dots hold more
                suggestions.
              </li>
              <li>
                On the dashed line, a shown 90% was accepted 9 times in 10.
                Below it, suggestions were overconfident; above it,
                underconfident.
              </li>
              <li>
                The curve refits as you review, so these dots drift toward the
                line over time.
              </li>
            </ul>
          </div>
        )}
      </div>
    </div>
  );
}

// --- Personal models -----------------------------------------------------------------

function modelLine(label: string, versions: ArtifactVersion[]): string {
  const current = versions.find((v) => v.active);
  if (current === undefined) return `${label}: not trained yet`;
  const accuracy =
    current.holdoutAccuracy === undefined
      ? ""
      : ` · ${percent(current.holdoutAccuracy)} on the holdout`;
  return `${label}: ${plural(current.examples, "example")}${accuracy} · trained ${ago(current.trainedAt)}`;
}

export function PersonalModels({
  metrics,
  status,
  onRetrain,
}: {
  metrics: AiMetrics;
  status: AiStatus | null;
  onRetrain: () => void;
}) {
  const busy = status?.retrain === "queued" || status?.retrain === "running";
  const { newLabels, retrainAfter } = metrics.models;
  const unavailable = status?.sidecar === "unavailable";
  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-col gap-0.5 text-[11.5px] text-fg-muted">
        <span>{modelLine("Category model", metrics.models.category)}</span>
        <span>{modelLine("Project model", metrics.models.project)}</span>
      </div>
      <p className="text-[11.5px] text-fg-faint">
        Retrains in the background after {retrainAfter} new reviews or nightly,
        only while the Mac is idle, on power, and not in Low Power Mode. A new
        model replaces the old one only if it does at least as well on held-out
        entries.{" "}
        <span className="text-fg-soft">
          {plural(newLabels, "new review")} since the last training.
        </span>
      </p>
      {status?.retrainNote !== undefined && (
        <p role="status" className="text-[11.5px] text-fg-soft">
          {status.retrainNote}
        </p>
      )}
      <button
        type="button"
        disabled={busy || unavailable}
        onClick={onRetrain}
        className="self-start rounded-lg border border-line bg-surface px-3 py-1.5 text-[12px] text-fg-muted hover:bg-surface-strong disabled:cursor-not-allowed disabled:opacity-50"
      >
        {status?.retrain === "running"
          ? "Retraining…"
          : status?.retrain === "queued"
            ? "Retrain queued…"
            : "Retrain now"}
      </button>
    </div>
  );
}
