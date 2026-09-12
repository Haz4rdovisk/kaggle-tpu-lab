import type { TpuPhase } from "../types/session";
import { WarningCircle } from "@phosphor-icons/react";

export interface PhaseMeta {
  label: string;
  tone: "gray" | "amber" | "blue" | "green" | "red";
  /** the status dot pulses while work is in flight */
  pulse: boolean;
}

export const PHASE_META: Record<TpuPhase, PhaseMeta> = {
  idle: { label: "IDLE", tone: "gray", pulse: false },
  queued: { label: "QUEUED", tone: "amber", pulse: true },
  provisioning: { label: "PROVISIONING", tone: "blue", pulse: true },
  starting: { label: "STARTING", tone: "blue", pulse: true },
  loadingWeights: { label: "LOADING WEIGHTS", tone: "blue", pulse: true },
  compiling: { label: "COMPILING", tone: "blue", pulse: true },
  healthy: { label: "HEALTHY", tone: "green", pulse: false },
  ready: { label: "READY", tone: "green", pulse: false },
  stopping: { label: "STOPPING", tone: "amber", pulse: true },
  stopped: { label: "STOPPED", tone: "gray", pulse: false },
  failed: { label: "FAILED", tone: "red", pulse: false },
};

interface Props {
  phase: TpuPhase;
  kernel: string | null;
  error: string | null;
}

export default function StatusHero({ phase, kernel, error }: Props) {
  const meta = PHASE_META[phase];
  return (
    <section className={`hero tone-${meta.tone}`} aria-live="polite">
      <div className="hero-phase-row">
        <span className={`dot ${meta.pulse ? "pulse" : ""}`} aria-hidden />
        <h1 className={`hero-phase tone-${meta.tone}`}>{meta.label}</h1>
      </div>
      <p className="hero-sub">
        {kernel ?? "No active session"}
        {kernel ? " · Qwen3.8-27B on TPU" : ""}
      </p>
      {error && (
        <div className="hero-error" role="alert">
          <WarningCircle size={13} weight="fill" aria-hidden />
          <span title={error}>{error}</span>
        </div>
      )}
    </section>
  );
}
