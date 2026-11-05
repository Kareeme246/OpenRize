import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { type Ref, type RefObject, useEffect, useRef, useState } from "react";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { BAR_OPENS_WINDOW_MENU } from "../lib/platform";

const EVENT_MAXIMIZE_BUTTON = "caption-maximize-button";

interface MaximizeButtonState {
  hovered: boolean;
  pressed: boolean;
}

type CaptionIcon = "minimize" | "maximize" | "restore" | "close";

const GLYPHS: Record<CaptionIcon, string> = {
  minimize: "",
  maximize: "",
  restore: "",
  close: "",
};

const ICON_FONT = '"Segoe Fluent Icons", "Segoe MDL2 Assets"';

/**
 * A stripped-down Windows image can lack the icon fonts, and a glyph measures
 * differently in the fallback font.
 */
const hasIconFont = (() => {
  let known: boolean | undefined;
  return (): boolean => {
    if (known === undefined) {
      try {
        const context = document.createElement("canvas").getContext("2d");
        const width = (family: string): number => {
          if (!context) return 0;
          context.font = `10px ${family}`;
          return context.measureText(GLYPHS.maximize).width;
        };
        known =
          !!context && width(`${ICON_FONT}, monospace`) !== width("monospace");
      } catch {
        known = false;
      }
    }
    return known;
  };
})();

interface ButtonRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

function reportMaximizeButton(rect: ButtonRect | null): Promise<void> {
  return invoke("set_maximize_button", { rect });
}

/**
 * Opens the system window menu on a right click on the bar's empty space. Rust
 * draws it because WebView2's own caption menu shows stale Maximize/Restore
 * states for a window it did not maximize.
 */
export function useWindowMenu(bar: RefObject<HTMLElement | null>): void {
  useEffect(() => {
    const element = bar.current;
    if (!BAR_OPENS_WINDOW_MENU || !element) return;
    const open = (event: MouseEvent) => {
      const target = event.target;
      if (
        target instanceof Element &&
        target.closest('button, a, input, [role="menu"]')
      ) {
        return;
      }
      event.preventDefault();
      invoke("show_system_menu").catch((cause: unknown) => {
        console.error("Could not open the window menu", cause);
      });
    };
    element.addEventListener("contextmenu", open);
    return () => element.removeEventListener("contextmenu", open);
  }, [bar]);
}

/**
 * Windows 11 window buttons. Snap Layouts only appears for a window that
 * answers a hit test, so a native window sits over Maximize and reports the
 * pointer back as an event (src-tauri/src/caption.rs).
 */
export function CaptionButtons() {
  const [maximized, setMaximized] = useState(false);
  const [active, setActive] = useState(() => document.hasFocus());
  const [pointer, setPointer] = useState<MaximizeButtonState>({
    hovered: false,
    pressed: false,
  });
  const maximizeRef = useRef<HTMLButtonElement>(null);

  const refreshMaximized = () => {
    getCurrentWindow()
      .isMaximized()
      .then(setMaximized)
      .catch(() => undefined);
  };
  useEffect(refreshMaximized, []);
  useTauriEvent<unknown>("tauri://resize", refreshMaximized);
  useTauriEvent<unknown>("tauri://focus", () => setActive(true));
  useTauriEvent<unknown>("tauri://blur", () => setActive(false));
  useTauriEvent<MaximizeButtonState>(EVENT_MAXIMIZE_BUTTON, setPointer);

  // Tells Rust where the maximize button is: on UI scale, window resize/zoom
  // and monitor changes.
  useEffect(() => {
    const button = maximizeRef.current;
    if (!button) return;
    let frame = 0;
    let sent = "";
    const send = () => {
      frame = 0;
      const box = button.getBoundingClientRect();
      const scale = window.devicePixelRatio;
      const rect: ButtonRect = {
        x: Math.round(box.left * scale),
        y: Math.round(box.top * scale),
        width: Math.round(box.width * scale),
        height: Math.round(box.height * scale),
      };
      const key = JSON.stringify(rect);
      if (key === sent) return;
      sent = key;
      reportMaximizeButton(rect).catch((cause: unknown) => {
        sent = "";
        console.error("Could not place the maximize button", cause);
      });
    };
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(send);
    };

    let stopWatchingScale = () => {};
    const watchScale = () => {
      const query = window.matchMedia(
        `(resolution: ${window.devicePixelRatio}dppx)`,
      );
      const changed = () => {
        schedule();
        watchScale();
      };
      query.addEventListener("change", changed, { once: true });
      stopWatchingScale = () => query.removeEventListener("change", changed);
    };

    const observer = new ResizeObserver(schedule);
    observer.observe(button);
    window.addEventListener("resize", schedule);
    watchScale();
    schedule();
    return () => {
      if (frame) cancelAnimationFrame(frame);
      observer.disconnect();
      window.removeEventListener("resize", schedule);
      stopWatchingScale();
      reportMaximizeButton(null).catch(() => undefined);
    };
  }, []);

  const current = getCurrentWindow();
  const toggleMaximize = () => {
    current.toggleMaximize().catch((cause: unknown) => {
      console.error("Could not maximize the window", cause);
    });
  };

  return (
    <div className="flex h-full shrink-0 self-stretch">
      <CaptionButton
        label="Minimize"
        icon="minimize"
        active={active}
        onClick={() => {
          current.minimize().catch((cause: unknown) => {
            console.error("Could not minimize the window", cause);
          });
        }}
      />
      <CaptionButton
        ref={maximizeRef}
        label={maximized ? "Restore" : "Maximize"}
        icon={maximized ? "restore" : "maximize"}
        active={active}
        hovered={pointer.hovered}
        pressed={pointer.pressed}
        onClick={toggleMaximize}
      />
      <CaptionButton
        label="Close"
        icon="close"
        active={active}
        danger
        onClick={() => {
          // Same close request as Alt+F4: Rust chooses between hiding to the
          // tray and quitting.
          current.close().catch((cause: unknown) => {
            console.error("Could not close the window", cause);
          });
        }}
      />
    </div>
  );
}

function CaptionButton({
  ref,
  label,
  icon,
  active,
  danger = false,
  hovered = false,
  pressed = false,
  onClick,
}: {
  ref?: Ref<HTMLButtonElement>;
  label: string;
  icon: CaptionIcon;
  active: boolean;
  danger?: boolean;
  hovered?: boolean;
  pressed?: boolean;
  onClick: () => void;
}) {
  const wash = danger
    ? "hover:bg-[#c42b1c] hover:text-white active:bg-[#c42b1c]/90 active:text-white/70"
    : "hover:bg-surface-strong active:bg-surface";
  // Over the maximize button the page sees no pointer; Rust reports it instead.
  const reported = pressed ? "bg-surface" : hovered ? "bg-surface-strong" : "";
  return (
    <button
      ref={ref}
      type="button"
      aria-label={label}
      onClick={onClick}
      className={`grid h-full w-[46px] shrink-0 place-items-center ${
        active ? "text-fg" : "text-fg-faint"
      } ${wash} ${reported}`}
    >
      <CaptionGlyph icon={icon} />
    </button>
  );
}

function CaptionGlyph({ icon }: { icon: CaptionIcon }) {
  if (hasIconFont()) {
    return (
      <span
        aria-hidden="true"
        className="select-none text-[10px] leading-none"
        style={{ fontFamily: ICON_FONT }}
      >
        {GLYPHS[icon]}
      </span>
    );
  }
  return (
    <svg
      viewBox="0 0 10 10"
      className="size-[10px]"
      fill="none"
      stroke="currentColor"
      strokeWidth={1}
      aria-hidden="true"
    >
      {icon === "minimize" && <path d="M0 5.5h10" />}
      {icon === "maximize" && (
        <rect x="0.5" y="0.5" width="9" height="9" rx="1" />
      )}
      {icon === "restore" && (
        <>
          <path d="M2.5 2.5V1.5a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-1" />
          <rect x="0.5" y="2.5" width="7" height="7" rx="1" />
        </>
      )}
      {icon === "close" && <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" />}
    </svg>
  );
}
