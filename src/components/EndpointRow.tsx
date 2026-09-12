import { Check, Copy } from "@phosphor-icons/react";
import type { SessionSnapshot } from "../types/session";
import { shortenEndpoint } from "../lib/format";
import { actions } from "../lib/store";

interface Props {
  snap: SessionSnapshot;
  copied: boolean;
}

export default function EndpointRow({ snap, copied }: Props) {
  const url = snap.endpoint;
  const badge = !url
    ? null
    : snap.endpointLive
      ? { text: "LIVE", tone: "green" }
      : { text: "RESERVED", tone: "blue" };

  return (
    <section className="card endpoint-card" aria-label="Endpoint">
      <div className="card-head">
        <span className="card-title">ENDPOINT</span>
        {badge && (
          <span className={`badge tone-${badge.tone}`}>{badge.text}</span>
        )}
      </div>
      <div className="endpoint-row">
        <code className="endpoint-url" title={url ?? undefined}>
          {shortenEndpoint(url)}
        </code>
        {url && (
          <button
            className="icon-btn"
            onClick={() => void actions.copyEndpoint()}
            title="Copy endpoint URL"
            aria-label="Copy endpoint URL"
          >
            {copied ? (
              <Check size={14} weight="bold" className="tone-green" aria-hidden />
            ) : (
              <Copy size={14} aria-hidden />
            )}
          </button>
        )}
      </div>
    </section>
  );
}
