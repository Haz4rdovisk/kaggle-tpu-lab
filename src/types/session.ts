/**
 * TypeScript mirrors of the Rust `SessionSnapshot` / `Settings` payloads.
 * Field names match the serde (camelCase) output exactly.
 */

export type TpuPhase =
  | "idle"
  | "verifying"
  | "queued"
  | "provisioning"
  | "starting"
  | "loadingWeights"
  | "compiling"
  | "healthy"
  | "ready"
  | "stopping"
  | "stopped"
  | "failed";

export type ModelId = "qwen38-27b" | "glm53-flash";

export const MODEL_LABELS: Record<ModelId, string> = {
  "qwen38-27b": "Qwen3.8-27B",
  "glm53-flash": "GLM-5.3-Flash",
};

export const MODEL_API_NAMES: Record<ModelId, string> = {
  "qwen38-27b": "qwen3.8-27b",
  "glm53-flash": "glm-5.3-flash",
};

export type NoteKind = "info" | "success" | "warn" | "error";

export interface ActivityNote {
  ts: number;
  text: string;
  kind: NoteKind;
}

export interface SessionSnapshot {
  phase: TpuPhase;
  model: ModelId | null;
  kaggleStatus: string | null;
  kernel: string | null;
  endpoint: string | null;
  endpointLive: boolean;
  readyAt: number | null;
  readyAtEstimated: boolean;
  queuedAt: number | null;
  allocatedAt: number | null;
  servingFrom: number | null;
  servingFromEstimated: boolean;
  keepaliveMin: number | null;
  uptimeSecs: number | null;
  remainingSecs: number | null;
  remainingEstimated: boolean;
  decodeTokS: number | null;
  maxModelLen: number | null;
  mtpTokens: number | null;
  textOnly: boolean | null;
  hasApiKey: boolean;
  ntfyReachable: boolean;
  activity: ActivityNote[];
  error: string | null;
  lastUpdate: number;
}

export interface QwenSettings {
  context: number;
  mtp: number;
  thinking: string;
  fastStart: boolean;
  textOnly: boolean;
  noAsyncScheduling: boolean;
}

export interface GlmSettings {
  context: number;
  streams: number;
  thinking: string;
  textOnly: boolean;
}

export interface Settings {
  projectRoot: string | null;
  model: ModelId;
  keepaliveMin: number;
  qwen: QwenSettings;
  glm: GlmSettings;
}

/** Phases in which "Start" is offered. */
export const CAN_START: readonly TpuPhase[] = ["idle", "stopped", "failed"];
/** Phases in which "Stop" is offered. */
export const CAN_STOP: readonly TpuPhase[] = [
  "verifying",
  "queued",
  "provisioning",
  "starting",
  "loadingWeights",
  "compiling",
  "healthy",
  "ready",
];
