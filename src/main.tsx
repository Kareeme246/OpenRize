import { getCurrentWindow } from "@tauri-apps/api/window";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Pulse from "./Pulse";
import "./styles/app.css";

// One bundle, two windows: the menu-bar panel is the `pulse` window.
const isPulse = getCurrentWindow().label === "pulse";
if (isPulse) document.documentElement.classList.add("pulse-window");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{isPulse ? <Pulse /> : <App />}</React.StrictMode>,
);
