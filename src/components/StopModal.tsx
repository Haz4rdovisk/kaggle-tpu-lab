import { Stop } from "@phosphor-icons/react";
import { useApp, actions } from "../lib/store";

export default function StopModal() {
  const { snapshot, busy, showStopModal } = useApp();
  if (!showStopModal) return null;
  const stopping = busy === "stop";

  return (
    <div
      className="overlay"
      role="dialog"
      aria-modal="true"
      aria-labelledby="stop-title"
      onClick={(e) => {
        if (e.target === e.currentTarget) actions.closeStopModal();
      }}
    >
      <div className="modal">
        <div className="modal-icon tone-red">
          <Stop size={18} weight="bold" aria-hidden />
        </div>
        <h2 id="stop-title" className="modal-title">
          Stop the TPU session?
        </h2>
        <p className="modal-body">
          This asks Kaggle to stop kernel{" "}
          <code className="mono">{snapshot?.kernel ?? "…"}</code>. The endpoint
          goes away immediately; weights stay cached, so the next start is
          faster.
        </p>
        <p className="modal-body muted">
          Stop is confirmed through the official launcher and can take a few
          minutes.
        </p>
        <div className="modal-actions">
          <button
            className="btn btn-ghost"
            onClick={actions.closeStopModal}
            disabled={stopping}
            autoFocus
          >
            Cancel
          </button>
          <button
            className="btn btn-danger"
            onClick={() => void actions.stop()}
            disabled={stopping}
          >
            {stopping ? "Stopping…" : "Stop session"}
          </button>
        </div>
      </div>
    </div>
  );
}
