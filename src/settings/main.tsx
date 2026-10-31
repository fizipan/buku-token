import React from "react";
import ReactDOM from "react-dom/client";
import "../index.css";
import { syncColorScheme } from "../lib/theme";
import { LogoSprite } from "../components/Logo";
import { Settings } from "./Settings";

syncColorScheme();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <LogoSprite />
    <Settings />
  </React.StrictMode>,
);
