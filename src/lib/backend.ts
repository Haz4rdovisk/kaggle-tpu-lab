/**
 * Backend bridge: typed Tauri command wrappers.
 *
 * When the app runs outside Tauri (plain `vite dev` in a browser — used for
 * visual inspection), a mock backend provides a realistic READY session so
 * the UI can be inspected without touching the real TPU.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ModelId, SessionSnapshot, Settings } from "../types/session";

export const isInTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

// ---------------------------------------------------------------------------
// Real (Tauri) implementation
// ---------------------------------------------------------------------------

const t = {
  getState: () => invoke<SessionSnapshot>("get_session_state"),
  refresh: () => invoke<SessionSnapshot>("refresh_status"),
  start: () => invoke<SessionSnapshot>("start_tpu"),
  stop: () => invoke<SessionSnapshot>("stop_tpu"),
  openKaggle: () => invoke<void>("open_kaggle"),
  copyEndpoint: () => invoke<void>("copy_endpoint"),
  copyApiKey: () => invoke<void>("copy_api_key"),
  copyModelName: () => invoke<void>("copy_model_name"),
  copyConnectionSetup: () => invoke<void>("copy_connection_setup"),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (s: Settings) => invoke<Settings>("save_settings", { settings: s }),
  onSessionUpdate: (cb: (s: SessionSnapshot) => void): Promise<UnlistenFn> =>
    listen<SessionSnapshot>("session-update", (e) => cb(e.payload)),
  onOpenStopConfirm: (cb: () => void): Promise<UnlistenFn> =>
    listen("open-stop-confirm", () => cb()),
  onOpenSettings: (cb: () => void): Promise<UnlistenFn> =>
    listen("open-settings", () => cb()),
};

// ---------------------------------------------------------------------------
// Mock implementation (browser preview only — never reaches Kaggle)
// ---------------------------------------------------------------------------

const MOCK_KERNEL = "lucastq/qwen38-tpu-serve";
const MOCK_GLM_KERNEL = "lucastq/glm53-tpu-serve";
const MOCK_ENDPOINT =
  "https://apollo-powerseller-pressing-instrument.trycloudflare.com/v1";
const MOCK_READY_AT = Math.floor(Date.now() / 1000) - (6 * 3600 + 42 * 60);
const MOCK_KEEPALIVE = 480;

function mockModel(): ModelId {
  if (typeof location === "undefined") return "qwen38-27b";
  const value = new URLSearchParams(location.search).get("model");
  return value === "glm53-flash" || value === "glm" ? "glm53-flash" : "qwen38-27b";
}

function mockSnapshot(): SessionSnapshot {
  const now = Math.floor(Date.now() / 1000);
  const model = mockModel();
  const kernel = model === "glm53-flash" ? MOCK_GLM_KERNEL : MOCK_KERNEL;
  const uptime = now - MOCK_READY_AT;
  const servingFrom = now - (5 * 3600 + 20 * 60);
  const remaining = MOCK_KEEPALIVE * 60 - (now - servingFrom);
  const phase =
    (typeof location !== "undefined" &&
      new URLSearchParams(location.search).get("mock")) ||
    "ready";

  const base: SessionSnapshot = {
    phase: "ready",
    model,
    kaggleStatus: "running",
    kernel,
    endpoint: MOCK_ENDPOINT,
    endpointLive: true,
    readyAt: MOCK_READY_AT,
    readyAtEstimated: false,
    queuedAt: MOCK_READY_AT - 42 * 60,
    allocatedAt: MOCK_READY_AT - 8 * 60,
    servingFrom,
    servingFromEstimated: true,
    keepaliveMin: MOCK_KEEPALIVE,
    uptimeSecs: uptime,
    remainingSecs: remaining,
    remainingEstimated: true,
    decodeTokS: 106.8,
    maxModelLen: 262144,
    mtpTokens: model === "qwen38-27b" ? 3 : null,
    textOnly: true,
    hasApiKey: true,
    ntfyReachable: true,
    activity: [
      { ts: now - 12 * 60, text: "Heartbeat (up ~6h 10m)", kind: "info" },
      { ts: now - 2 * 3600, text: "Benchmark 106.8 tok/s (decode)", kind: "info" },
      { ts: MOCK_READY_AT, text: "READY — session live", kind: "success" },
      { ts: MOCK_READY_AT - 9 * 60, text: "Compiling kernels (JIT)", kind: "info" },
      { ts: MOCK_READY_AT - 38 * 60, text: "Loading weights (sharded)", kind: "info" },
      { ts: MOCK_READY_AT - 41 * 60, text: "Endpoint reserved", kind: "info" },
      { ts: MOCK_READY_AT - 42 * 60, text: "Kernel queued on Kaggle", kind: "info" },
    ],
    error: null,
    lastUpdate: now,
  };

  // Phase variants for previewing every state in the browser.
  switch (phase) {
    case "idle":
      return {
        ...base, phase: "idle", kernel: null, endpoint: null, endpointLive: false,
        readyAt: null, queuedAt: null, allocatedAt: null, servingFrom: null,
        uptimeSecs: null, remainingSecs: null, decodeTokS: null, maxModelLen: null,
        mtpTokens: null, hasApiKey: false, ntfyReachable: true,
        activity: [],
      };
    case "queued":
      return {
        ...base, phase: "queued", endpointLive: false, readyAt: null, servingFrom: null,
        uptimeSecs: null, remainingSecs: null, decodeTokS: null, maxModelLen: null, mtpTokens: null,
        queuedAt: now - 3 * 60,
        activity: [{ ts: now, text: "Kernel queued on Kaggle", kind: "info" }],
      };
    case "compiling":
      return {
        ...base, phase: "compiling", endpointLive: false, readyAt: null, servingFrom: null,
        uptimeSecs: null, remainingSecs: null, decodeTokS: null, mtpTokens: null,
        activity: [
          { ts: now - 30, text: "Compiling kernels (JIT)", kind: "info" },
          { ts: now - 600, text: "Endpoint reserved", kind: "info" },
        ],
      };
    case "failed":
      return {
        ...base, phase: "failed", endpointLive: false, readyAt: null, servingFrom: null,
        uptimeSecs: null, remainingSecs: null, decodeTokS: null, mtpTokens: null,
        queuedAt: now - 8 * 60,
        ntfyReachable: false,
        error: "vLLM exited with code 1 (CUDA OOM during compile)",
        activity: [
          { ts: now - 15, text: "vLLM exited with code 1 (CUDA OOM during compile)", kind: "error" },
          { ts: now - 900, text: "Compiling kernels (JIT)", kind: "info" },
        ],
      };
    case "stopped":
      return {
        ...base, phase: "stopped", endpointLive: false, readyAt: null, servingFrom: null,
        uptimeSecs: null, remainingSecs: null, decodeTokS: null, mtpTokens: null,
        queuedAt: now - 8 * 60,
        activity: [{ ts: now, text: "TPU session stopped", kind: "info" }],
      };
    default:
      return base;
  }
}

const mock = {
  async getState(): Promise<SessionSnapshot> {
    return mockSnapshot();
  },
  async refresh(): Promise<SessionSnapshot> {
    return mockSnapshot();
  },
  async start(): Promise<SessionSnapshot> {
    const s = mockSnapshot();
    s.phase = "queued";
    s.activity = [
      { ts: Math.floor(Date.now() / 1000), text: "Kernel submitted", kind: "info" },
      ...s.activity,
    ];
    return s;
  },
  async stop(): Promise<SessionSnapshot> {
    const s = mockSnapshot();
    s.phase = "stopped";
    s.endpointLive = false;
    s.uptimeSecs = null;
    s.remainingSecs = null;
    s.activity = [
      { ts: Math.floor(Date.now() / 1000), text: "TPU session stopped", kind: "info" },
      ...s.activity,
    ];
    return s;
  },
  async openKaggle(): Promise<void> {
    window.open(`https://www.kaggle.com/code/${MOCK_KERNEL}`, "_blank");
  },
  async copyEndpoint(): Promise<void> {
    const endpoint = mockModel() === "glm53-flash" ? MOCK_ENDPOINT.replace(/\/v1$/, "") : MOCK_ENDPOINT;
    await navigator.clipboard.writeText(endpoint.endsWith("/v1") ? endpoint : `${endpoint}/v1`);
  },
  async copyApiKey(): Promise<void> {
    await navigator.clipboard.writeText("sk-preview-only");
  },
  async copyModelName(): Promise<void> {
    await navigator.clipboard.writeText(mockModel() === "glm53-flash" ? "glm-5.3-flash" : "qwen3.8-27b");
  },
  async copyConnectionSetup(): Promise<void> {
    const model = mockModel() === "glm53-flash" ? "glm-5.3-flash" : "qwen3.8-27b";
    const endpoint = mockModel() === "glm53-flash" ? MOCK_ENDPOINT.replace(/\/v1$/, "") : MOCK_ENDPOINT;
    const base = endpoint.endsWith("/v1") ? endpoint : `${endpoint}/v1`;
    await navigator.clipboard.writeText(`OPENAI_BASE_URL=${base}\nOPENAI_API_KEY=sk-preview-only\nOPENAI_MODEL=${model}`);
  },
  async getSettings(): Promise<Settings> {
    return {
      projectRoot: null,
      model: mockModel(),
      keepaliveMin: 480,
      qwen: {
        context: 262144,
        mtp: 3,
        thinking: "xhigh",
        fastStart: true,
        textOnly: true,
        noAsyncScheduling: false,
      },
      glm: {
        context: 262144,
        streams: 4,
        thinking: "low",
        textOnly: false,
      },
    };
  },
  async saveSettings(s: Settings): Promise<Settings> {
    return s;
  },
  onSessionUpdate(cb: (s: SessionSnapshot) => void): Promise<UnlistenFn> {
    // `?static=1` freezes the preview (no periodic re-render) — used for
    // screenshot capture where the page must reach an idle state.
    if (typeof location !== "undefined" && location.search.includes("static=1")) {
      cb(mockSnapshot());
      return Promise.resolve(() => {});
    }
    const id = window.setInterval(() => cb(mockSnapshot()), 5000);
    return Promise.resolve(() => window.clearInterval(id));
  },
  onOpenStopConfirm(): Promise<UnlistenFn> {
    return Promise.resolve(() => {});
  },
  onOpenSettings(): Promise<UnlistenFn> {
    return Promise.resolve(() => {});
  },
};

export const backend = isInTauri() ? t : mock;
export const previewMode = !isInTauri();
