export const STALE_AFTER_MS = 2 * 60 * 1000;

// Age of the rate in whole seconds. Clamped at zero so clock skew never shows a negative age.
export function rateAgeSeconds(rateAsOf: string, nowMs: number): number {
  return Math.max(0, Math.round((nowMs - new Date(rateAsOf).getTime()) / 1000));
}

// Stale when the rate is older than two minutes, or when the last quote request failed.
export function isRateStale(rateAsOf: string, nowMs: number, lastRequestFailed: boolean): boolean {
  if (lastRequestFailed) return true;
  return nowMs - new Date(rateAsOf).getTime() > STALE_AFTER_MS;
}
