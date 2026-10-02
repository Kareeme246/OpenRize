import { useCallback, useEffect, useState } from "react";
import { type Board, EMPTY_BOARD, type ExtensionStatus } from "../lib/agents";
import * as api from "../lib/api";
import { useTauriEvent } from "./useTauriEvent";

/** The live flight board, adopted from `agents-changed`. */
export function useAgentBoard(): Board {
  const [board, setBoard] = useState<Board>(EMPTY_BOARD);
  useEffect(() => {
    api.agentBoard().then(setBoard, () => undefined);
  }, []);
  useTauriEvent<Board>(api.AGENTS_CHANGED, setBoard);
  return board;
}

/** The extensions Settings lists, refreshed when one connects or drops. */
export function useExtensions(): {
  extensions: ExtensionStatus[];
  refresh: () => void;
} {
  const [extensions, setExtensions] = useState<ExtensionStatus[]>([]);
  const refresh = useCallback(() => {
    api.listExtensions().then(setExtensions, () => undefined);
  }, []);
  useEffect(refresh, [refresh]);
  useTauriEvent<undefined>(api.EXTENSIONS_CHANGED, refresh);
  return { extensions, refresh };
}
