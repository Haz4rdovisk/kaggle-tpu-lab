import { Check, Copy, Key, PlugsConnected } from "@phosphor-icons/react";
import { actions, useApp } from "../lib/store";
import { MODEL_API_NAMES, type SessionSnapshot } from "../types/session";

function openAiBaseUrl(endpoint: string | null): string {
  if (!endpoint) return "—";
  const root = endpoint.replace(/\/+$/, "");
  return root.endsWith("/v1") ? root : `${root}/v1`;
}

interface Props {
  snap: SessionSnapshot;
}

export default function ConnectCard({ snap }: Props) {
  const { connectionCopied } = useApp();
  const ready = snap.endpointLive && !!snap.endpoint && snap.hasApiKey && !!snap.model;
  const baseUrl = openAiBaseUrl(snap.endpoint);
  const model = snap.model ? MODEL_API_NAMES[snap.model] : "—";

  return (
    <section className="card connect-card" aria-label="Connect to this model">
      <div className="card-head">
        <div className="connect-title-wrap">
          <span className="card-title">CONNECT</span>
          <span className="card-sub">OpenAI compatible</span>
        </div>
        <span className={`badge tone-${ready ? "green" : "gray"}`}>
          <PlugsConnected size={12} weight="bold" aria-hidden />
          {ready ? "READY" : "WAITING"}
        </span>
      </div>

      <div className="connect-fields">
        <div className="connect-field">
          <span className="connect-label">Base URL</span>
          <code className="connect-value">{baseUrl}</code>
          <button
            className="icon-btn"
            type="button"
            disabled={!snap.endpoint}
            onClick={() => void actions.copyConnection("endpoint")}
            title="Copy OpenAI base URL"
            aria-label="Copy OpenAI base URL"
          >
            {connectionCopied === "endpoint" ? <Check size={14} weight="bold" className="tone-green" /> : <Copy size={14} />}
          </button>
        </div>

        <div className="connect-field">
          <span className="connect-label">Model</span>
          <code className="connect-value">{model}</code>
          <button
            className="icon-btn"
            type="button"
            disabled={!snap.model}
            onClick={() => void actions.copyConnection("model")}
            title="Copy model name"
            aria-label="Copy model name"
          >
            {connectionCopied === "model" ? <Check size={14} weight="bold" className="tone-green" /> : <Copy size={14} />}
          </button>
        </div>

        <div className="connect-field">
          <span className="connect-label">API key</span>
          <code className="connect-value">{snap.hasApiKey ? "••••••••••••••••" : "—"}</code>
          <button
            className="icon-btn"
            type="button"
            disabled={!snap.hasApiKey}
            onClick={() => void actions.copyConnection("key")}
            title="Copy API key"
            aria-label="Copy API key"
          >
            {connectionCopied === "key" ? <Check size={14} weight="bold" className="tone-green" /> : <Key size={14} />}
          </button>
        </div>
      </div>

      <button
        className="btn btn-ghost btn-block connect-copy-setup"
        type="button"
        disabled={!ready}
        onClick={() => void actions.copyConnection("setup")}
      >
        {connectionCopied === "setup" ? <Check size={14} weight="bold" /> : <Copy size={14} />}
        {connectionCopied === "setup" ? "Setup copied" : "Copy setup"}
      </button>
    </section>
  );
}
