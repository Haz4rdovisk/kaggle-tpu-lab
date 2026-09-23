import { GearSix } from "@phosphor-icons/react";
import { setState } from "../lib/store";
import { MODEL_LABELS, type Settings } from "../types/session";

interface Props {
  settings: Settings;
}

function contextLabel(context: number): string {
  if (context === 262144) return "262K context";
  if (context === 131072) return "131K context";
  return `${context.toLocaleString()} context`;
}

export default function LaunchProfile({ settings }: Props) {
  const isQwen = settings.model === "qwen38-27b";
  const profile = isQwen ? settings.qwen : settings.glm;

  return (
    <section className="launch-profile" aria-label="Selected TPU model">
      <div className="launch-profile-main">
        <div className="launch-profile-heading">
          <span className="launch-profile-kicker">Next model</span>
          <strong className="launch-profile-model">{MODEL_LABELS[settings.model]}</strong>
        </div>
        <button
          className="btn btn-ghost btn-sm launch-profile-change"
          onClick={() => setState({ showSettings: true })}
          type="button"
        >
          <GearSix size={13} aria-hidden />
          Change
        </button>
      </div>

      <div className="launch-profile-meta" aria-label="Selected launch profile">
        <span>{contextLabel(profile.context)}</span>
        <span>{profile.thinking} reasoning</span>
        <span>{isQwen ? `MTP ${settings.qwen.mtp}` : `${settings.glm.streams} streams`}</span>
      </div>

      <p className="launch-profile-hint">
        {isQwen ? "Qwen profile" : "GLM profile"}
      </p>
    </section>
  );
}
