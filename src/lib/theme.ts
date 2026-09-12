/**
 * Kaggle TPU Companion — theme engine (light/dark/system).
 *
 * Design tokens live in CSS only (src/styles.css). This module owns the
 * theme preference state and its application to the document.
 */
import { useSyncExternalStore } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { previewMode } from "./backend";

export type Theme = "light" | "dark";
export type ThemePref = "system" | "light" | "dark";

const STORAGE_KEY = "ktpu.theme";
const MEDIA_QUERY = "(prefers-color-scheme: dark)";

/** Stored preference, or "system" when absent/invalid/unavailable. */
export function getThemePref(): ThemePref {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === "light" || stored === "dark") return stored;
  } catch {
    // Storage unavailable — fall through to system.
  }
  return "system";
}

/** Persist the preference (cleared for "system") and apply + notify. */
export function setThemePref(p: ThemePref): void {
  try {
    if (p === "system") localStorage.removeItem(STORAGE_KEY);
    else localStorage.setItem(STORAGE_KEY, p);
  } catch {
    // Persistence is best-effort; the theme still applies for the session.
  }
  applyTheme(resolveTheme(p));
  emit();
}

/** OS-level theme. Defaults to dark (the app's identity) when unknown. */
export function systemTheme(): Theme {
  if (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia(MEDIA_QUERY).matches
  ) {
    return "dark";
  }
  return "light";
}

/** Concrete theme for a preference ("system" follows the OS). */
export function resolveTheme(p: ThemePref): Theme {
  return p === "system" ? systemTheme() : p;
}

/** Apply the theme to the document. Never touches localStorage. */
export function applyTheme(t: Theme): void {
  if (typeof document !== "undefined") {
    document.documentElement.dataset.theme = t;
    document.documentElement.style.colorScheme = t;
  }
  if (!previewMode) {
    try {
      const win = getCurrentWindow();
      void win.setBackgroundColor(t === "dark" ? [24, 24, 24] : [255, 255, 255]).catch(() => {});
      void win.setTheme(t).catch(() => {});
    } catch {
      /* janela nativa indisponivel */
    }
  }
}

// ---------------------------------------------------------------------------
// Subscription (same useSyncExternalStore pattern as src/lib/store.ts)
// ---------------------------------------------------------------------------

type Listener = () => void;

const listeners = new Set<Listener>();
let mediaWatched = false;

function readSnapshot(): { pref: ThemePref; theme: Theme } {
  const pref = getThemePref();
  return { pref, theme: resolveTheme(pref) };
}

let snapshot = readSnapshot();

function emit() {
  snapshot = readSnapshot();
  for (const l of listeners) l();
}

function ensureMediaWatcher() {
  if (mediaWatched) return;
  if (typeof window === "undefined" || typeof window.matchMedia !== "function")
    return;
  mediaWatched = true;
  window.matchMedia(MEDIA_QUERY).addEventListener("change", () => {
    if (getThemePref() === "system") {
      applyTheme(systemTheme());
      emit();
    }
  });
}

export function subscribeTheme(cb: () => void): () => void {
  ensureMediaWatcher();
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

export function useTheme(): { pref: ThemePref; theme: Theme } {
  return useSyncExternalStore(
    subscribeTheme,
    () => snapshot,
    () => ({ pref: "system" as ThemePref, theme: "dark" as Theme }),
  );
}
