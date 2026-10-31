import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";
import { syncColorScheme } from "./lib/theme";

syncColorScheme();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
