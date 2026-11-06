import { useEffect, useRef, useState } from "react";
import * as api from "../lib/api";
import type { Invoice, InvoiceDraftInput } from "../lib/types";

export const PREVIEW_DEBOUNCE_MS = 400;

export interface InvoicePreview {
  pdf: Uint8Array | null;
  quote: Invoice | null;
  pending: boolean;
  problem: string | null;
}

/**
 * Renders the draft through Rust after a quiet period; stale results are
 * discarded and the last good PDF stays up. A rejected draft sets `problem`.
 */
export function useInvoicePreview(
  draft: InvoiceDraftInput | null,
  delayMs = PREVIEW_DEBOUNCE_MS,
): InvoicePreview {
  const [state, setState] = useState<InvoicePreview>({
    pdf: null,
    quote: null,
    pending: draft !== null,
    problem: null,
  });
  const latest = useRef(0);
  const key = draft === null ? null : JSON.stringify(draft);

  useEffect(() => {
    const request = ++latest.current;
    if (key === null) {
      setState((prev) => ({
        ...prev,
        pending: false,
        problem: "Finish the highlighted line items to preview.",
      }));
      return;
    }
    setState((prev) => ({ ...prev, pending: true }));
    const timer = window.setTimeout(async () => {
      try {
        const input = JSON.parse(key) as InvoiceDraftInput;
        const [quote, pdf] = await Promise.all([
          api.quoteInvoice(input),
          api.renderInvoicePreview(input),
        ]);
        if (request !== latest.current) return;
        setState({ pdf, quote, pending: false, problem: null });
      } catch (cause) {
        if (request !== latest.current) return;
        setState((prev) => ({
          ...prev,
          pending: false,
          problem: api.describeError(cause),
        }));
      }
    }, delayMs);
    return () => window.clearTimeout(timer);
  }, [key, delayMs]);

  return state;
}
