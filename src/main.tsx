import { getCurrentWindow } from "@tauri-apps/api/window";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { ToastProvider } from "./hooks/useToast";
import * as api from "./lib/api";
import Pulse from "./Pulse";
import Reminder from "./Reminder";
import "./styles/app.css";

// Capture unhandled window errors and promise rejections into local crash reports
window.addEventListener("error", (event) => {
  api
    .recordClientCrash(
      "frontend_error",
      event.message || "Unknown window error",
      undefined,
      {
        filename: event.filename,
        lineno: event.lineno,
        colno: event.colno,
      },
    )
    .catch(() => undefined);
});

window.addEventListener("unhandledrejection", (event) => {
  const reason = event.reason;
  const message =
    reason instanceof Error
      ? reason.message
      : String(reason ?? "Unhandled promise rejection");
  const stack = reason instanceof Error ? reason.stack : undefined;
  api
    .recordClientCrash("unhandled_rejection", message, undefined, { stack })
    .catch(() => undefined);
});

// One bundle, three windows: the menu-bar panel is the `pulse` window and the
// top-right break panel is the `reminder` window.
const label = getCurrentWindow().label;
if (label === "pulse") document.documentElement.classList.add("pulse-window");
if (label === "reminder")
  document.documentElement.classList.add("reminder-window");

function Root() {
  if (label === "pulse") return <Pulse />;
  if (label === "reminder") return <Reminder />;
  return (
    <ToastProvider>
      <App />
    </ToastProvider>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <Root />
    </ErrorBoundary>
  </React.StrictMode>,
);
