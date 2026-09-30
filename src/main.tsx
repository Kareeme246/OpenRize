import { getCurrentWindow } from "@tauri-apps/api/window";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Pulse from "./Pulse";
import Reminder from "./Reminder";
import "./styles/app.css";

// One bundle, three windows: the menu-bar panel is the `pulse` window and the
// top-right break panel is the `reminder` window.
const label = getCurrentWindow().label;
if (label === "pulse") document.documentElement.classList.add("pulse-window");
if (label === "reminder")
  document.documentElement.classList.add("reminder-window");

function Root() {
  if (label === "pulse") return <Pulse />;
  if (label === "reminder") return <Reminder />;
  return <App />;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
