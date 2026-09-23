import { MODEL_LABELS, type ModelId, type TpuPhase } from "../types/session";
import { WarningCircle } from "@phosphor-icons/react";

export interface PhaseMeta {
  label: string;
  tone: "gray" | "amber" | "blue" | "green" | "red";
  /** the status dot pulses while work is in flight */
  pulse: boolean;
}

export const PHASE_META: Record<TpuPhase, PhaseMeta> = {
  idle: { label: "IDLE", tone: "gray", pulse: false },
  verifying: { label: "VERIFYING", tone: "amber", pulse: true },
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
  model: ModelId | null;
  error: string | null;
}

export default function StatusHero({ phase, kernel, model, error }: Props) {
  const meta = PHASE_META[phase];
  const modelRole = kernel ? "Active" : "Selected";
  return (
    <section className={`hero tone-${meta.tone}`} aria-live="polite">
      <div className="hero-top-row">
        <div className="hero-phase-row">
          <span className={`dot ${meta.pulse ? "pulse" : ""}`} aria-hidden />
          <h1 className={`hero-phase tone-${meta.tone}`}>{meta.label}</h1>
        </div>
        {model && (
          <div className="hero-model-badge">
            <span>{modelRole}</span>
            <strong>{MODEL_LABELS[model]}</strong>
          </div>
        )}
      </div>
      <p className="hero-sub">
        {kernel ?? "No active session"}
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
