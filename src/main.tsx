import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { applyTheme, getThemePref, resolveTheme } from "./lib/theme";
import "./styles.css";

applyTheme(resolveTheme(getThemePref()));

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
