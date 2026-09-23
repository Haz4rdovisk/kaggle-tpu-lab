import { useEffect, useState } from "react";
import type { SessionSnapshot } from "../types/session";
import { formatDuration, formatInt, formatTokS } from "../lib/format";

interface Props {
  snap: SessionSnapshot;
}

function Metric({
  label,
  value,
  est,
  muted,
}: {
  label: string;
  value: string;
  est?: boolean;
  muted?: boolean;
}) {
  return (
    <div className={`metric ${muted ? "muted" : ""}`}>
      <span className="metric-label">{label}</span>
      <span className="metric-value">
        {value}
        {est && <sup className="est" title="estimated">~</sup>}
      </span>
    </div>
  );
}

export default function Metrics({ snap }: Props) {
  const active = snap.uptimeSecs != null;
  const [nowSecs, setNowSecs] = useState(() => Math.floor(Date.now() / 1000));
  const queued = snap.phase === "queued";
  const terminal = snap.phase === "idle" || snap.phase === "stopped" || snap.phase === "failed";
  const waiting = snap.queuedAt != null && snap.readyAt == null && !terminal;
  useEffect(() => {
    if (!waiting) return;
    const id = window.setInterval(() => setNowSecs(Math.floor(Date.now() / 1000)), 1000);
    return () => window.clearInterval(id);
  }, [waiting]);
  const end = snap.readyAt ?? nowSecs;
  const waitSecs = snap.queuedAt != null ? end - snap.queuedAt : null;
  return (
    <section className="metrics" aria-label="Session metrics">
      <div className="metrics-primary">
        <Metric
          label="UPTIME"
          value={formatDuration(snap.uptimeSecs)}
          muted={!active}
        />
        <Metric
          label="REMAINING"
          value={formatDuration(snap.remainingSecs)}
          est={snap.remainingEstimated}
          muted={!active}
        />
        <Metric
          label={queued ? "QUEUE" : "WAITED FOR"}
          value={formatDuration(waitSecs)}
          muted={waitSecs == null}
        />
      </div>
      <div className="metrics-secondary">
        <Metric label="DECODE" value={snap.decodeTokS != null ? `${formatTokS(snap.decodeTokS)} t/s` : "—"} muted={snap.decodeTokS == null} />
        <Metric label="CONTEXT" value={formatInt(snap.maxModelLen)} muted={snap.maxModelLen == null} />
      </div>
    </section>
  );
}
