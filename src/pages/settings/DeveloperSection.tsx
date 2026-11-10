import { useState } from "react";
import { Toggle } from "../../components/SegmentedControl";
import { HAS_PERMISSIONS } from "../../lib/platform";
import { DevToolsSetting } from "./DevToolsSetting";
import { SettingGroup, SettingRow } from "./SettingParts";

const ADVANCED_DEVELOPER_MODE_KEY = "openrize.advancedDeveloperMode";

/** A per-device convenience in localStorage; off until switched on. */
function readAdvancedDeveloperMode(): boolean {
  try {
    return window.localStorage.getItem(ADVANCED_DEVELOPER_MODE_KEY) === "1";
  } catch {
    return false;
  }
}

function writeAdvancedDeveloperMode(on: boolean): void {
  try {
    window.localStorage.setItem(ADVANCED_DEVELOPER_MODE_KEY, on ? "1" : "0");
  } catch {
    // Storage can be blocked; the switch still works for this session.
  }
}

/**
 * Dev builds only (the caller gates on `DEV_TOOLS`). One switch that reveals
 * the developer tools, so a dev build's Settings otherwise looks shipped.
 */
export function DeveloperSection() {
  const [enabled, setEnabled] = useState(readAdvancedDeveloperMode);

  const change = (on: boolean): void => {
    setEnabled(on);
    writeAdvancedDeveloperMode(on);
  };

  return (
    <div className="flex flex-col gap-4">
      <SettingGroup title="Advanced Developer Mode">
        <SettingRow
          title="Advanced Developer Mode"
          description="Show developer tools in this dev build. Kept on this device."
        >
          <Toggle
            checked={enabled}
            label="Advanced Developer Mode"
            onChange={change}
          />
        </SettingRow>
      </SettingGroup>
      {enabled && HAS_PERMISSIONS && <DevToolsSetting />}
    </div>
  );
}
