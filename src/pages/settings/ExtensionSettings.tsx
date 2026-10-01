import { Toggle } from "../../components/SegmentedControl";
import { useExtensions } from "../../hooks/useAgents";
import { useSettings } from "../../hooks/useSettings";
import { connectionLabel, type ExtensionStatus } from "../../lib/agents";
import { SettingGroup, SettingRow } from "./SettingParts";

/** Integrations that are planned but not built yet, listed so the view reads as the full set. */
const LATER = [
  {
    title: "Claude Code hooks",
    description: "Exact working and waiting states straight from the agent",
  },
  {
    title: "Codex notify",
    description: "Turn-finished signals from Codex",
  },
] as const;

function ExtensionRow({ status }: { status: ExtensionStatus }) {
  const { settings, update } = useSettings();
  const explicit = settings.extensions[status.id] !== undefined;

  const choose = (enabled: boolean): void => {
    update({ extensions: { ...settings.extensions, [status.id]: enabled } });
  };
  const reset = (): void => {
    const rest = { ...settings.extensions };
    delete rest[status.id];
    update({ extensions: rest });
  };

  return (
    <SettingRow
      title={status.name}
      description={`${connectionLabel(status)}${
        status.detail === null ? "" : ` (${status.detail})`
      }`}
    >
      <div className="flex shrink-0 items-center gap-3">
        {explicit && (
          <button
            type="button"
            onClick={reset}
            className="text-[12px] font-medium text-accent hover:underline"
          >
            Reset to auto
          </button>
        )}
        <Toggle
          checked={status.enabled}
          label={`${status.name} extension`}
          onChange={choose}
        />
      </div>
    </SettingRow>
  );
}

/**
 * Agent sources. A tool found on this Mac is switched on by itself; a switch
 * the person has flipped by hand stays where they put it, through restarts and
 * re-detection, until they reset it to auto.
 */
export function ExtensionSettingsGroups() {
  const { extensions } = useExtensions();
  return (
    <>
      <SettingGroup title="Agent sources">
        {extensions.length === 0 ? (
          <div className="px-4 py-3 text-[12px] text-fg-muted">
            Looking for tools…
          </div>
        ) : (
          extensions.map((status) => (
            <ExtensionRow key={status.id} status={status} />
          ))
        )}
      </SettingGroup>
      <SettingGroup title="Coming later">
        {LATER.map((item) => (
          <SettingRow
            key={item.title}
            title={item.title}
            description={item.description}
            status="not-implemented"
          />
        ))}
      </SettingGroup>
      <p className="px-1 text-[12px] leading-relaxed text-fg-muted">
        Extensions read only what each tool already knows: whether an agent is
        working, waiting, or done, and which folder it is in. Terminal content
        is never read or stored.
      </p>
    </>
  );
}
