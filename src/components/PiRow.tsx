import type { ReactElement } from "react";
import { ArrowsClockwise, Check, Warning, X } from "@phosphor-icons/react";
import type { PiState } from "../types/session";
import { useApp, actions } from "../lib/store";

const PI_META: Record<PiState, { label: string; tone: string; icon: ReactElement }> = {
  notConfigured: { label: "NOT CONFIGURED", tone: "gray", icon: <X size={12} aria-hidden /> },
  synced: { label: "SYNCED", tone: "green", icon: <Check size={12} weight="bold" aria-hidden /> },
  stale: { label: "STALE", tone: "amber", icon: <Warning size={12} weight="fill" aria-hidden /> },
  syncFailed: { label: "SYNC FAILED", tone: "red", icon: <X size={12} aria-hidden /> },
};

export default function PiRow() {
  const { snapshot, busy } = useApp();
  const st: PiState = snapshot?.piStatus ?? "notConfigured";
  const meta = PI_META[st];
  const syncing = busy === "sync";

  return (
    <section className="card pi-card" aria-label="Pi provider sync">
      <div className="card-head">
        <span className="card-title">PI PROVIDER</span>
        <span className={`badge tone-${meta.tone}`}>
          {meta.icon}
          {meta.label}
        </span>
      </div>
      <p className="pi-hint">
        {st === "synced" && "kaggle-tpu points at the live endpoint."}
        {st === "stale" && "kaggle-tpu points at a different endpoint."}
        {st === "syncFailed" && "Last sync failed — files were rolled back."}
        {st === "notConfigured" && "Pi config not found on this machine."}
      </p>
      <button
        className="btn btn-ghost btn-sm"
        disabled={syncing || st === "notConfigured"}
        onClick={() => void actions.syncPi()}
      >
        <ArrowsClockwise size={13} className={syncing ? "spin" : ""} aria-hidden />
        {syncing ? "Syncing…" : st === "syncFailed" ? "Retry sync" : "Sync now"}
      </button>
    </section>
  );
}
