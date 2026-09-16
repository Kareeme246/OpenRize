import type { ReactNode } from "react";

/*
 * Every control here is a stub. 
 * delete the badge (not the row) when a control lands.
 */
function SettingRow({
  title,
  description,
}: {
  title: string;
  description: string;
}) {
  return (
    <div className="flex items-center justify-between gap-4 border-b border-white/5 px-4 py-3 last:border-b-0">
      <div className="min-w-0">
        <div className="text-[13px] font-medium">{title}</div>
        <div className="text-[11.5px] text-white/40">{description}</div>
      </div>
      <span className="shrink-0 rounded-full border border-white/10 bg-white/5 px-2 py-0.5 font-mono text-[9.5px] uppercase tracking-wider text-white/40">
        Not implemented
      </span>
    </div>
  );
}

function SettingGroup({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="flex flex-col gap-2">
      <h2 className="px-1 font-mono text-[10px] uppercase tracking-wider text-white/35">
        {title}
      </h2>
      <div className="rounded-xl border border-white/10 bg-linear-to-b from-white/5 to-white/1">
        {children}
      </div>
    </section>
  );
}

export function Settings() {
  return (
    <main className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-5.5">
      <header>
        <h1 className="text-[15px] font-semibold">Settings</h1>
        <div className="font-mono text-[10.5px] text-white/35">
          ⌘, opens this · nothing below is wired up yet
        </div>
      </header>

      <SettingGroup title="Appearance">
        <SettingRow title="Theme" description="Dark, light, or follow the system" />
        <SettingRow title="Accent colour" description="Recolour highlights across the app" />
      </SettingGroup>

      <SettingGroup title="General">
        <SettingRow title="Launch at login" description="Start OpenRize when you log in" />
        <SettingRow
          title="Global shortcut"
          description="Open OpenRize from any app (the ⌘, shortcut above works in-app only)"
        />
        <SettingRow title="Menu bar" description="Keep a tray icon with quick controls" />
      </SettingGroup>

      <SettingGroup title="Notifications">
        <SettingRow
          title="Long-run reminders"
          description="Ping me when a tracker has been running unusually long"
        />
      </SettingGroup>

      <SettingGroup title="Data">
        <SettingRow title="Storage location" description="Where trackers.json lives on disk" />
        <SettingRow title="Export trackers" description="Download every tracker and its time" />
        <SettingRow title="Reset all data" description="Delete every tracker and start clean" />
      </SettingGroup>

      <SettingGroup title="Work hours">
        <SettingRow
          title="Expected hours"
          description="Your baseline work week, used for utilization and capacity metrics"
        />
        <SettingRow
          title="Count toward Work Hours"
          description="Choose which categories count as work rather than personal time"
        />
      </SettingGroup>
    </main>
  );
}
