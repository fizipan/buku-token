import { describe, expect, it } from "vitest";
import { isExpiredWindow, isStaleWindow, remainingPct, weeklyPace } from "./quota";

const DAY = 86_400_000;
const NOW = new Date("2026-10-30T10:00:00Z");
const at = (ms: number) => new Date(NOW.getTime() + ms).toISOString();

describe("weeklyPace", () => {
  it("is 0 right after a reset and 100 right before the next", () => {
    expect(weeklyPace(at(7 * DAY), NOW)).toBe(0);
    expect(weeklyPace(at(0), NOW)).toBe(100);
  });
  it("is the elapsed share of the 7 day window", () => {
    expect(weeklyPace(at(3.5 * DAY), NOW)).toBeCloseTo(50, 5);
    expect(weeklyPace(at(2 * DAY), NOW)).toBeCloseTo((5 / 7) * 100, 5);
  });
  it("clamps to 0..100", () => {
    expect(weeklyPace(at(10 * DAY), NOW)).toBe(0);
    expect(weeklyPace(at(-DAY), NOW)).toBe(100);
  });
});

describe("remainingPct", () => {
  it("rounds and clamps", () => {
    expect(remainingPct(38)).toBe(62);
    expect(remainingPct(37.6)).toBe(62);
    expect(remainingPct(120)).toBe(0);
    expect(remainingPct(-3)).toBe(100);
  });
});

describe("isStaleWindow", () => {
  const w = (over: object) => ({ id: "seven_day", label: "x", usedPct: 1, resetsAt: at(DAY), observedAt: at(-DAY / 2), expired: false, ...over });
  it("is stale when expired or observed more than 24h ago", () => {
    expect(isStaleWindow(w({}), NOW)).toBe(false);
    expect(isStaleWindow(w({ expired: true }), NOW)).toBe(true);
    expect(isStaleWindow(w({ observedAt: at(-DAY - 1000) }), NOW)).toBe(true);
    expect(isStaleWindow(w({ observedAt: at(-DAY + 1000) }), NOW)).toBe(false);
  });
});

describe("isExpiredWindow", () => {
  const w = (over: object) => ({ id: "seven_day", label: "Mingguan", usedPct: 40, resetsAt: at(DAY), observedAt: at(-1000), expired: false, ...over });
  it("trusts the backend flag", () => {
    expect(isExpiredWindow(w({ expired: true }), NOW)).toBe(true);
  });
  it("catches a reset passed since the data was read", () => {
    expect(isExpiredWindow(w({ resetsAt: at(-1) }), NOW)).toBe(true);
    expect(isStaleWindow(w({ resetsAt: at(-1) }), NOW)).toBe(true);
    expect(isExpiredWindow(w({}), NOW)).toBe(false);
  });
});
