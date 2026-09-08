/** Duration / clock formatting (keeps parity with the Rust formatters). */

/** "6h 42m" · "42m" · "38s" · "—" for null. Compact for tray-style rows. */
export function formatDuration(secs: number | null | undefined): string {
  if (secs == null || Number.isNaN(secs)) return "—";
  const s = Math.max(0, Math.floor(secs));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = s % 60;
  if (h > 0) return m > 0 ? `${h}h ${m}m` : `${h}h`;
  if (m > 0) return r > 0 ? `${m}m ${r}s` : `${m}m`;
  return `${r}s`;
}

/** "06:42:05" local, 24h, from a unix-seconds timestamp. */
export function formatClock(ts: number | null | undefined): string {
  if (ts == null) return "—";
  const d = new Date(ts * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

/** "262,144" — thousands separators. */
export function formatInt(n: number | null | undefined): string {
  if (n == null) return "—";
  return n.toLocaleString("en-US");
}

/** "106.8" — one decimal. */
export function formatTokS(n: number | null | undefined): string {
  if (n == null) return "—";
  return n.toFixed(1);
}

/** Short endpoint for tight rows: host + "/v1". */
export function shortenEndpoint(url: string | null | undefined): string {
  if (!url) return "—";
  try {
    const u = new URL(url);
    return `${u.host}${u.pathname !== "/" ? u.pathname : ""}`;
  } catch {
    return url.length > 48 ? `…${url.slice(-47)}` : url;
  }
}
