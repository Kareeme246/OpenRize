import { useEffect, useState } from "react";

/**
 * The current time for live displays (now lines, a recording entry's end),
 * updated every 15 s and only while the app is visible.
 */
export function useMinuteClock(): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    let timer: number | undefined;
    const sync = (): void => {
      window.clearInterval(timer);
      if (document.visibilityState === "visible") {
        setNow(Date.now());
        timer = window.setInterval(() => setNow(Date.now()), 15_000);
      }
    };
    document.addEventListener("visibilitychange", sync);
    sync();
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", sync);
    };
  }, []);
  return now;
}
