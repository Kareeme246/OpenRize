import { useEffect, useRef, useState } from "react";
import * as api from "../lib/api";
import type { Invoice, InvoiceDraftInput } from "../lib/types";

/** Quiet time after the last edit before a preview is requested. */
export const PREVIEW_DEBOUNCE_MS = 400;

export interface InvoicePreview {
  /** The last successfully rendered draft PDF; kept while a newer one loads. */
  pdf: Uint8Array | null;
  /** The same draft priced by Rust: line amounts, total, due date. */
  quote: Invoice | null;
  /** A render is queued or in flight. */
  pending: boolean;
  /** Why the current draft cannot be previewed, if it cannot. */
  problem: string | null;
}

/**
 * Renders the editor's draft through Rust as the user types.
 *
 * Every change (re)starts a quiet-period timer, so typing never renders per
 * keystroke; a request that finishes after a newer one was issued is
 * discarded, and the previous good PDF and quote stay on screen while the next
 * renders, so the paper never blanks or flickers. A draft that Rust rejects
 * (say, no lines yet) reports `problem` and leaves the last document alone.
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
