import { useState } from "react";
import { RotateCcw, X } from "lucide-react";
import type { Settings } from "../types/session";
import { useApp, actions } from "../lib/store";

const DEFAULTS: Settings = {
  projectRoot: null,
  context: 262144,
  mtp: 3,
  thinking: "xhigh",
  fastStart: true,
  textOnly: true,
  keepaliveMin: 480,
};

export default function SettingsModal() {
  const { settings, showSettings, busy } = useApp();
  const [draft, setDraft] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);

  if (!showSettings) return null;
  const base = draft ?? settings ?? DEFAULTS;
  const saving = busy != null;

  function patch(p: Partial<Settings>) {
    setDraft({ ...base, ...p });
    setError(null);
  }

  function submit() {
    if (!base) return;
    // Client-side sanity mirrors the Rust validation exactly.
    if (base.context !== 131072 && base.context !== 262144)
      return setError("Context must be 131,072 or 262,144.");
    if (Number.isNaN(base.mtp) || base.mtp < 0 || base.mtp > 5)
      return setError("MTP must be between 0 and 5.");
    if (!["xhigh", "medium", "low"].includes(base.thinking))
      return setError("Reasoning effort must be xhigh, medium or low.");
    if (Number.isNaN(base.keepaliveMin) || base.keepaliveMin < 30 || base.keepaliveMin > 540)
      return setError("Keepalive must be between 30 and 540 minutes.");
    void actions.saveSettings(base);
  }

  return (
    <div
      className="overlay"
      role="dialog"
      aria-modal="true"
      aria-labelledby="settings-title"
      onClick={(e) => {
        if (e.target === e.currentTarget) actions.closeSettings();
      }}
    >
      <div className="modal modal-wide">
        <div className="modal-head-row">
          <h2 id="settings-title" className="modal-title">
            Session settings
          </h2>
          <button
            className="icon-btn"
            onClick={actions.closeSettings}
            aria-label="Close settings"
          >
            <X size={15} aria-hidden />
          </button>
        </div>
        <p className="modal-body muted">
          Applied to the next Start. The running session is never modified.
          The profile always includes the fixed flags
          <code className="mono"> --reasoning-effort</code>,
          <code className="mono"> --text-only</code> and
          <code className="mono"> --fast-start</code> (tools are never
          disabled).
        </p>

        <div className="field-grid">
          <label className="field">
            <span>Max model len</span>
            <select
              value={base.context}
              onChange={(e) => patch({ context: Number(e.target.value) })}
            >
              <option value={131072}>131,072</option>
              <option value={262144}>262,144</option>
            </select>
          </label>
          <label className="field">
            <span>MTP tokens (0–5)</span>
            <input
              type="number"
              min={0}
              max={5}
              value={base.mtp}
              onChange={(e) => patch({ mtp: Number(e.target.value) })}
            />
          </label>
          <label className="field">
            <span>Reasoning effort</span>
            <select
              value={base.thinking}
              onChange={(e) => patch({ thinking: e.target.value })}
            >
              <option value="xhigh">xhigh</option>
              <option value="medium">medium</option>
              <option value="low">low</option>
            </select>
          </label>
          <label className="field">
            <span>Keepalive (min, 30–540)</span>
            <input
              type="number"
              min={30}
              max={540}
              value={base.keepaliveMin}
              onChange={(e) => patch({ keepaliveMin: Number(e.target.value) })}
            />
          </label>
          <label className="field field-check">
            <input
              type="checkbox"
              checked={base.textOnly}
              onChange={(e) => patch({ textOnly: e.target.checked })}
            />
            <span>Text only</span>
          </label>
          <label className="field field-check">
            <input
              type="checkbox"
              checked={base.fastStart}
              onChange={(e) => patch({ fastStart: e.target.checked })}
            />
            <span>Fast start</span>
          </label>
        </div>

        {error && (
          <div className="form-error" role="alert">
            {error}
          </div>
        )}

        <div className="modal-actions">
          <button
            className="btn btn-ghost"
            onClick={() => {
              setDraft({ ...DEFAULTS });
              setError(null);
            }}
          >
            <RotateCcw size={13} aria-hidden />
            Reset
          </button>
          <button
            className="btn btn-ghost"
            onClick={actions.closeSettings}
            disabled={saving}
          >
            Cancel
          </button>
          <button className="btn btn-primary" onClick={submit} disabled={saving}>
            {saving ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
