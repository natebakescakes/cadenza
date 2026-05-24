// Human-friendly formatting helpers. Pure, dependency-free, null-tolerant.

/** Milliseconds → compact human string: "340ms", "1.2s", "3.4s". */
export function formatMs(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms) || ms <= 0) return "—";
  if (ms < 1000) return `${Math.round(ms)}ms`;
  const s = ms / 1000;
  if (s < 10) return `${s.toFixed(1)}s`;
  if (s < 60) return `${Math.round(s)}s`;
  const m = Math.floor(s / 60);
  const rem = Math.round(s % 60);
  return `${m}m ${rem}s`;
}

/** Round a WPM value for display; em-dash for empty. */
export function formatWpm(wpm: number | null | undefined): string {
  if (wpm == null || !Number.isFinite(wpm)) return "—";
  return Math.round(wpm).toString();
}

/** Integer with thousands separators; "0" stays "0". */
export function formatNumber(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return "—";
  return Math.round(n).toLocaleString("en-US");
}

/** Fraction (0..1) → "73%". */
export function formatPercent(
  frac: number | null | undefined,
  digits = 0,
): string {
  if (frac == null || !Number.isFinite(frac)) return "—";
  return `${(frac * 100).toFixed(digits)}%`;
}

/** Epoch ms (or seconds) → relative time: "just now", "3m ago", "2d ago". */
export function formatRelative(ts: number | null | undefined): string {
  if (ts == null || !Number.isFinite(ts) || ts <= 0) return "never";
  // Tolerate seconds-based epochs (heuristic: < year 2001 in ms).
  const ms = ts < 1e11 ? ts * 1000 : ts;
  const diff = Date.now() - ms;
  if (diff < 0) return "just now";
  const sec = Math.floor(diff / 1000);
  if (sec < 45) return "just now";
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const day = Math.floor(hr / 24);
  if (day < 30) return `${day}d ago`;
  const mo = Math.floor(day / 30);
  if (mo < 12) return `${mo}mo ago`;
  return `${Math.floor(mo / 12)}y ago`;
}

/** Clamp helper for progress bars etc. */
export function clamp(n: number, min = 0, max = 1): number {
  return Math.min(max, Math.max(min, n));
}
