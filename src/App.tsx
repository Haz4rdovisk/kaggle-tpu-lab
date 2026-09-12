import { useEffect } from "react";
import { ArrowSquareOut, ArrowsClockwise, GearSix, Minus, Moon, Play, Stop, Sun, X } from "@phosphor-icons/react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import StatusHero from "./components/StatusHero";
import Metrics from "./components/Metrics";
import EndpointRow from "./components/EndpointRow";
import PiRow from "./components/PiRow";
import ActivityLog from "./components/ActivityLog";
import StopModal from "./components/StopModal";
import SettingsModal from "./components/SettingsModal";
import { useApp, actions, setState } from "./lib/store";
import { setThemePref, useTheme } from "./lib/theme";
import { previewMode } from "./lib/backend";
import { CAN_START, CAN_STOP, type SessionSnapshot } from "./types/session";

function Header({ snap }: { snap: SessionSnapshot | null }) {
  const { busy } = useApp();
  const { theme } = useTheme();
  const refreshing = busy === "refresh";
  const win = previewMode ? null : getCurrentWindow();
  return (
    <header className="app-header" data-tauri-drag-region>
      <div className="brand">
        <span className="brand-mark" aria-hidden>K</span>
        <span className="brand-name">Kaggle TPU Companion</span>
        {previewMode && <span className="badge tone-amber preview-badge">PREVIEW</span>}
      </div>
      <div className="header-actions">
        {snap?.kernel && (
          <button
            className="icon-btn"
            onClick={actions.openKaggle}
            title="Open kernel on Kaggle"
            aria-label="Open kernel on Kaggle"
          >
            <ArrowSquareOut size={14} aria-hidden />
          </button>
        )}
        <button
          className="icon-btn"
          onClick={() => void actions.refresh()}
          title="Refresh now"
          aria-label="Refresh now"
        >
          <ArrowsClockwise size={14} className={refreshing ? "spin" : ""} aria-hidden />
        </button>
        <button
          className="icon-btn"
          onClick={() => setThemePref(theme === "dark" ? "light" : "dark")}
          title={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
          aria-label={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
        >
          {theme === "dark" ? <Sun size={14} aria-hidden /> : <Moon size={14} aria-hidden />}
        </button>
        <button
          className="icon-btn"
          onClick={() => setState({ showSettings: true })}
          title="Settings"
          aria-label="Settings"
        >
          <GearSix size={14} aria-hidden />
        </button>
        {win && (
          <div className="win-controls">
            <button
              className="win-btn"
              onClick={() => void win.minimize()}
              title="Minimize"
              aria-label="Minimize window"
            >
              <Minus size={14} aria-hidden />
            </button>
            <button
              className="win-btn win-btn-close"
              onClick={() => void win.close()}
              title="Hide to tray"
              aria-label="Hide to tray"
            >
              <X size={14} aria-hidden />
            </button>
          </div>
        )}
      </div>
    </header>
  );
}

function Footer({ snap }: { snap: SessionSnapshot | null }) {
  const { busy } = useApp();
  const phase = snap?.phase;
  const canStart = phase != null && CAN_START.includes(phase);
  const canStop = phase != null && CAN_STOP.includes(phase);
  const working = busy === "start" || busy === "stop";

  return (
    <footer className="app-footer">
      {canStart && (
        <button
          className="btn btn-primary btn-block"
          disabled={working}
          onClick={() => void actions.start()}
        >
          <Play size={15} weight="bold" aria-hidden />
          {busy === "start" ? "Starting…" : "Start TPU session"}
        </button>
      )}
      {canStop && (
        <button
          className="btn btn-danger btn-block"
          disabled={working}
          onClick={() => setState({ showStopModal: true })}
        >
          <Stop size={15} weight="bold" aria-hidden />
          {busy === "stop" ? "Stopping…" : "Stop session"}
        </button>
      )}
      {!canStart && !canStop && (
        <div className="footer-idle">
          {busy === "start" ? "Waiting for Kaggle to accept the kernel…" : "—"}
        </div>
      )}
    </footer>
  );
}

export default function App() {
  const { snapshot, errorBanner, endpointCopied } = useApp();

  useEffect(() => {
    void actions.init();
  }, []);

  return (
    <div className="app">
      <Header snap={snapshot} />
      <main className="app-body">
        {errorBanner && (
          <div className="error-banner" role="alert">
            <span>{errorBanner}</span>
            <button
              className="icon-btn"
              onClick={actions.dismissError}
              aria-label="Dismiss error"
            >
              <X size={13} aria-hidden />
            </button>
          </div>
        )}
        {snapshot ? (
          <>
            <StatusHero
              phase={snapshot.phase}
              kernel={snapshot.kernel}
              error={snapshot.error}
            />
            <Metrics snap={snapshot} />
            <EndpointRow snap={snapshot} copied={endpointCopied} />
            <PiRow />
            <ActivityLog activity={snapshot.activity} />
          </>
        ) : (
          <div className="loading-state">
            <ArrowsClockwise size={18} className="spin" aria-hidden />
            <span>Reading session state…</span>
          </div>
        )}
      </main>
      {snapshot && <Footer snap={snapshot} />}
      <StopModal />
      <SettingsModal />
    </div>
  );
}
