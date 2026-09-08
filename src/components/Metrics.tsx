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
      </div>
      <div className="metrics-secondary">
        <Metric label="DECODE" value={snap.decodeTokS != null ? `${formatTokS(snap.decodeTokS)} t/s` : "—"} muted={snap.decodeTokS == null} />
        <Metric label="CONTEXT" value={formatInt(snap.maxModelLen)} muted={snap.maxModelLen == null} />
        <Metric label="KEEPALIVE" value={snap.keepaliveMin != null ? formatDuration(snap.keepaliveMin * 60) : "—"} muted={snap.keepaliveMin == null} />
      </div>
    </section>
  );
}
