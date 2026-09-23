/**
 * Minimal external store (useSyncExternalStore).
 * Single source of truth for the panel; backend events push into it.
 */
import { useSyncExternalStore } from "react";
import type { SessionSnapshot, Settings } from "../types/session";
import { backend } from "./backend";

export type Busy = null | "start" | "stop" | "refresh";
export type ConnectionCopy = null | "endpoint" | "model" | "key" | "setup";

export interface AppState {
  snapshot: SessionSnapshot | null;
  settings: Settings | null;
  busy: Busy;
  errorBanner: string | null;
  showStopModal: boolean;
  showSettings: boolean;
  connectionCopied: ConnectionCopy;
}

const initial: AppState = {
  snapshot: null,
  settings: null,
  busy: null,
  errorBanner: null,
  showStopModal: false,
  showSettings: false,
  connectionCopied: null,
};

let state: AppState = initial;
const listeners = new Set<() => void>();

function emit() {
  for (const l of listeners) l();
}

export function getState(): AppState {
  return state;
}

export function setState(patch: Partial<AppState>) {
  state = { ...state, ...patch };
  emit();
}

export function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

export function useApp(): AppState {
  return useSyncExternalStore(subscribe, getState, getState);
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

let copyResetTimer: number | undefined;

async function withBusy<T>(
  busy: NonNullable<Busy>,
  fn: () => Promise<T>,
  onOk?: (v: T) => void,
): Promise<T | undefined> {
  setState({ busy });
  try {
    const v = await fn();
    onOk?.(v);
    return v;
  } catch (e) {
    setState({ errorBanner: e instanceof Error ? e.message : String(e) });
    return undefined;
  } finally {
    setState({ busy: null });
  }
}

export const actions = {
  async init() {
    const [snap, settings] = await Promise.all([
      backend.getState().catch(() => null),
      backend.getSettings().catch(() => null),
    ]);
    setState({ snapshot: snap, settings });
    await backend.onSessionUpdate((s) => setState({ snapshot: s }));
    await backend.onOpenStopConfirm(() => setState({ showStopModal: true }));
    await backend.onOpenSettings(() => setState({ showSettings: true }));
  },

  refresh() {
    return withBusy("refresh", () => backend.refresh(), (s) =>
      setState({ snapshot: s }),
    );
  },

  start() {
    return withBusy("start", () => backend.start(), (s) =>
      setState({ snapshot: s, showStopModal: false }),
    );
  },

  stop() {
    return withBusy("stop", () => backend.stop(), (s) =>
      setState({ snapshot: s, showStopModal: false }),
    );
  },

  async copyConnection(kind: Exclude<ConnectionCopy, null>) {
    try {
      if (kind === "endpoint") await backend.copyEndpoint();
      if (kind === "model") await backend.copyModelName();
      if (kind === "key") await backend.copyApiKey();
      if (kind === "setup") await backend.copyConnectionSetup();
      setState({ connectionCopied: kind });
      if (copyResetTimer) window.clearTimeout(copyResetTimer);
      copyResetTimer = window.setTimeout(
        () => setState({ connectionCopied: null }),
        1600,
      );
    } catch (e) {
      setState({ errorBanner: e instanceof Error ? e.message : String(e) });
    }
  },

  openKaggle() {
    void backend.openKaggle();
  },

  saveSettings(s: Settings) {
    return withBusy("refresh", () => backend.saveSettings(s), (saved) =>
      setState({ settings: saved, showSettings: false }),
    );
  },

  dismissError() {
    setState({ errorBanner: null });
  },

  closeStopModal() {
    setState({ showStopModal: false });
  },

  closeSettings() {
    setState({ showSettings: false });
  },
};
