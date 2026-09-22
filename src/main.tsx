import React from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { McpControl } from "./components/McpControl";
import { KernelTuning } from "./components/KernelTuning";
const tuningId = new URLSearchParams(location.search).get("tuning");
import "./style.css";
createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <McpControl>{tuningId ? <KernelTuning id={tuningId} /> : <App />}</McpControl>
  </React.StrictMode>,
);
