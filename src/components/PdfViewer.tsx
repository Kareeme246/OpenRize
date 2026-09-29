import type {
  PDFDocumentLoadingTask,
  PDFDocumentProxy,
  RenderTask,
} from "pdfjs-dist/legacy/build/pdf.mjs";
import { useEffect, useRef, useState } from "react";

type PdfJs = typeof import("pdfjs-dist/legacy/build/pdf.mjs");

let pdfjs: Promise<PdfJs> | undefined;

/**
 * pdf.js is large and only invoices use it, so it loads on first use, as its
 * own chunk. Its worker ships inside the app bundle; nothing is fetched from
 * the network.
 */
function loadPdfJs(): Promise<PdfJs> {
  pdfjs ??= (async () => {
    const [library, worker] = await Promise.all([
      import("pdfjs-dist/legacy/build/pdf.mjs"),
      import("pdfjs-dist/legacy/build/pdf.worker.min.mjs?url"),
    ]);
    library.GlobalWorkerOptions.workerSrc = worker.default;
    return library;
  })();
  return pdfjs;
}

const ZOOM_STEPS = [0.5, 0.75, 1, 1.25, 1.5, 2] as const;
const PAGE_GAP = 16;
const GUTTER = 24;

interface PdfViewerProps {
  /** The PDF to show; the previous one stays until this one is ready. */
  bytes: Uint8Array | null;
  /** Dims the paper while a newer render is on its way. */
  stale?: boolean;
  /** Names the document for assistive tech, e.g. "Invoice preview". */
  label: string;
  /** Shown while there is nothing to display yet. */
  placeholder?: string;
}

/**
 * Displays PDF bytes as paper: pdf.js draws each page to a canvas, sharp on
 * retina, fit to width with zoom steps. Pages render lazily as they near the
 * viewport, and every render is finished off-screen and swapped in whole, so a
 * live preview updates without flicker. The bytes are the real document;
 * this component never lays out or prices anything.
 */
export function PdfViewer({
  bytes,
  stale = false,
  label,
  placeholder = "Preparing preview...",
}: PdfViewerProps) {
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [zoom, setZoom] = useState<number>(1);
  const [width, setWidth] = useState(0);
  const scroller = useRef<HTMLDivElement>(null);
  const current = useRef<PDFDocumentLoadingTask | null>(null);

  useEffect(() => {
    if (bytes === null) return;
    let cancelled = false;
    let task: PDFDocumentLoadingTask | undefined;
    loadPdfJs()
      .then((library) => {
        if (cancelled) return undefined;
        // pdf.js transfers the buffer to its worker, so hand it a copy.
        task = library.getDocument({ data: bytes.slice() });
        return task.promise;
      })
      .then((loaded) => {
        if (loaded === undefined) return;
        if (cancelled) {
          void task?.destroy();
          return;
        }
        const previous = current.current;
        current.current = task ?? null;
        setDoc(loaded);
        setError(null);
        // Give the pages a moment to swap before releasing the old document.
        if (previous) window.setTimeout(() => void previous.destroy(), 1000);
      })
      .catch((cause: unknown) => {
        if (!cancelled) {
          setError(cause instanceof Error ? cause.message : String(cause));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [bytes]);

  useEffect(
    () => () => {
      void current.current?.destroy();
      current.current = null;
    },
    [],
  );

  useEffect(() => {
    const element = scroller.current;
    if (element === null) return;
    let timer: number | undefined;
    const measure = (): void => setWidth(element.clientWidth);
    measure();
    const observer = new ResizeObserver(() => {
      window.clearTimeout(timer);
      timer = window.setTimeout(measure, 80);
    });
    observer.observe(element);
    return () => {
      window.clearTimeout(timer);
      observer.disconnect();
    };
  }, []);

  const zoomIndex = ZOOM_STEPS.indexOf(zoom as (typeof ZOOM_STEPS)[number]);
  const pageWidth = Math.max(200, (width - GUTTER * 2) * zoom);

  return (
    <div className="flex h-full min-h-0 flex-col bg-inset-soft">
      <div className="flex h-9 shrink-0 items-center justify-between border-line border-b px-3 text-[11.5px] text-fg-soft">
        <span aria-live="polite">
          {doc
            ? `${doc.numPages} ${doc.numPages === 1 ? "page" : "pages"}`
            : ""}
          {stale && <span className="ml-2 text-fg-faint">Updating...</span>}
        </span>
        <div className="flex items-center gap-1">
          <button
            type="button"
            aria-label="Zoom out"
            disabled={zoomIndex <= 0}
            onClick={() => setZoom(ZOOM_STEPS[Math.max(0, zoomIndex - 1)])}
            className="flex size-6 items-center justify-center rounded-md border border-line bg-panel hover:bg-surface disabled:opacity-40"
          >
            -
          </button>
          <span className="w-10 text-center tabular-nums">
            {Math.round(zoom * 100)}%
          </span>
          <button
            type="button"
            aria-label="Zoom in"
            disabled={zoomIndex === ZOOM_STEPS.length - 1}
            onClick={() =>
              setZoom(
                ZOOM_STEPS[Math.min(ZOOM_STEPS.length - 1, zoomIndex + 1)],
              )
            }
            className="flex size-6 items-center justify-center rounded-md border border-line bg-panel hover:bg-surface disabled:opacity-40"
          >
            +
          </button>
        </div>
      </div>
      <div ref={scroller} className="min-h-0 flex-1 overflow-auto">
        {error ? (
          <p role="alert" className="p-6 text-[12px] text-danger">
            The preview could not be displayed: {error}
          </p>
        ) : doc === null ? (
          <p className="p-6 text-center text-[12px] text-fg-faint">
            {placeholder}
          </p>
        ) : (
          <section
            aria-label={label}
            className={`flex flex-col items-center py-6 transition-opacity duration-150 ${stale ? "opacity-70" : ""}`}
            style={{ gap: PAGE_GAP, minWidth: pageWidth + GUTTER * 2 }}
          >
            {Array.from({ length: doc.numPages }, (_, index) => (
              <PdfPage
                // biome-ignore lint/suspicious/noArrayIndexKey: pages are positional and never reorder
                key={index}
                doc={doc}
                pageNumber={index + 1}
                width={pageWidth}
              />
            ))}
          </section>
        )}
      </div>
    </div>
  );
}

function PdfPage({
  doc,
  pageNumber,
  width,
}: {
  doc: PDFDocumentProxy;
  pageNumber: number;
  width: number;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const holder = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(width * (11 / 8.5));
  const [visible, setVisible] = useState(pageNumber === 1);

  useEffect(() => {
    const element = holder.current;
    if (element === null) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry?.isIntersecting) setVisible(true);
      },
      { rootMargin: "900px 0px" },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    let cancelled = false;
    let task: RenderTask | undefined;
    (async () => {
      const page = await doc.getPage(pageNumber);
      const base = page.getViewport({ scale: 1 });
      const cssScale = width / base.width;
      if (cancelled) return;
      setHeight(base.height * cssScale);
      if (!visible) return;
      const density = window.devicePixelRatio || 1;
      const viewport = page.getViewport({ scale: cssScale * density });
      const buffer = document.createElement("canvas");
      buffer.width = Math.floor(viewport.width);
      buffer.height = Math.floor(viewport.height);
      task = page.render({ canvas: buffer, viewport });
      await task.promise;
      const target = canvas.current;
      if (cancelled || target === null) return;
      target.width = buffer.width;
      target.height = buffer.height;
      target.getContext("2d")?.drawImage(buffer, 0, 0);
    })().catch((cause: unknown) => {
      // A render cancelled by a newer one is expected, not a failure.
      if (
        (cause as { name?: string })?.name !== "RenderingCancelledException"
      ) {
        console.error("Invoice page failed to render", cause);
      }
    });
    return () => {
      cancelled = true;
      task?.cancel();
    };
  }, [doc, pageNumber, width, visible]);

  return (
    <div
      ref={holder}
      className="shrink-0 overflow-hidden rounded-[3px] bg-white shadow-[0_2px_14px_rgba(0,0,0,0.35)]"
      style={{ width, height }}
    >
      <canvas
        ref={canvas}
        role="img"
        aria-label={`Page ${pageNumber} of ${doc.numPages}`}
        style={{ width, height }}
      />
    </div>
  );
}
