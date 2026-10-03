import { useLayoutEffect, useRef } from "react";
import { readViewport, rememberViewport } from "./viewport";

export function useRememberedScroll(key: string) {
  const viewport = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const element = viewport.current;
    const saved = readViewport(key);
    if (element && saved) {
      element.scrollTop = saved.top;
      element.scrollLeft = saved.left;
    }
  }, [key]);
  const onScroll = (): void => {
    const element = viewport.current;
    if (element)
      rememberViewport(key, {
        top: element.scrollTop,
        left: element.scrollLeft,
      });
  };
  return { viewport, onScroll };
}
