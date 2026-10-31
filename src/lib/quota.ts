import type { QuotaWindow } from "./api";

const DAY = 24 * 60 * 60_000;
const WEEK = 7 * DAY;

/** Share of the 7 day window already elapsed (0..100): the "fair" usage so far. */
export function weeklyPace(resetsAt: string, now: Date): number {
  const left = new Date(resetsAt).getTime() - now.getTime();
  return Math.min(100, Math.max(0, ((WEEK - left) / WEEK) * 100));
}

export const remainingPct = (usedPct: number): number => Math.round(Math.min(100, Math.max(0, 100 - usedPct)));

/** Past its reset: flagged by the backend, or the reset passed since the data was read. */
export const isExpiredWindow = (w: QuotaWindow, now: Date): boolean =>
  w.expired || new Date(w.resetsAt).getTime() < now.getTime();

/** Expired, or last observed more than 24 h ago. */
export const isStaleWindow = (w: QuotaWindow, now: Date): boolean =>
  isExpiredWindow(w, now) || now.getTime() - new Date(w.observedAt).getTime() > DAY;
