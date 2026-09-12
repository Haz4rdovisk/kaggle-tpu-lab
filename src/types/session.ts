/**
 * TypeScript mirrors of the Rust `SessionSnapshot` / `Settings` payloads.
 * Field names match the serde (camelCase) output exactly.
 */

export type TpuPhase =
  | "idle"
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

export type PiState = "notConfigured" | "synced" | "stale" | "syncFailed";

export type NoteKind = "info" | "success" | "warn" | "error";

export interface ActivityNote {
  ts: number;
  text: string;
  kind: NoteKind;
}

export interface SessionSnapshot {
  phase: TpuPhase;
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
  piStatus: PiState;
  ntfyReachable: boolean;
  activity: ActivityNote[];
  error: string | null;
  lastUpdate: number;
}

export interface Settings {
  projectRoot: string | null;
  context: number;
  mtp: number;
  thinking: string;
  fastStart: boolean;
  textOnly: boolean;
  keepaliveMin: number;
}

/** Phases in which "Start" is offered. */
export const CAN_START: readonly TpuPhase[] = ["idle", "stopped", "failed"];
/** Phases in which "Stop" is offered. */
export const CAN_STOP: readonly TpuPhase[] = [
  "queued",
  "provisioning",
  "starting",
  "loadingWeights",
  "compiling",
  "healthy",
  "ready",
];
