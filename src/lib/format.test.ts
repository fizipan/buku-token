import { describe, expect, it } from "vitest";
import { clock, count, dayLabel, dayMonth, duration, idr, multiple, pct, price, shortDate, tokens, usd, usd0 } from "./format";

describe("format (id-ID)", () => {
  it("usd", () => {
    expect(usd(15.25)).toBe("$15,25");
    expect(usd(0)).toBe("$0,00");
    expect(usd(null)).toBe("–");
  });
  it("usd0", () => {
    expect(usd0(1240)).toBe("$1.240");
    expect(usd0(null)).toBe("–");
  });
  it("idr", () => {
    expect(idr(15.25, 16300)).toBe("Rp248.575");
    expect(idr(1240, 16300)).toBe("Rp20,21 jt");
    expect(idr(0, 16300)).toBe("Rp0");
    expect(idr(999_999.6, 1)).toBe("Rp1 jt");
    expect(idr(999_999.4, 1)).toBe("Rp999.999");
    expect(idr(1_240_000_000, 1)).toBe("Rp1,24 M");
    expect(idr(999_995_000, 1)).toBe("Rp1 M");
  });
  it("tokens", () => {
    expect(tokens(1_240_000)).toBe("1,24 jt");
    expect(tokens(612_000)).toBe("612 rb");
    expect(tokens(980)).toBe("980");
    expect(tokens(0)).toBe("0");
    expect(tokens(5_200_000_000)).toBe("5,2 M");
    expect(tokens(1_240_000_000)).toBe("1,24 M");
    expect(tokens(999_999)).toBe("1 jt");
    expect(tokens(999_500)).toBe("1 jt");
    expect(tokens(999_499)).toBe("999 rb");
    expect(tokens(999_995_000)).toBe("1 M");
    expect(tokens(999_994_000)).toBe("999,99 jt");
  });
  it("duration", () => {
    expect(duration(38)).toBe("38 mnt");
    expect(duration(72)).toBe("1j 12m");
    expect(duration(120)).toBe("2j");
  });
  it("pct", () => {
    expect(pct(62)).toBe("62%");
    expect(pct(61.6)).toBe("62%");
  });
  it("clock uses local time with a dot", () => {
    expect(clock(new Date(2026, 9, 30, 12, 40).toISOString())).toBe("12.40");
    expect(clock(new Date(2026, 9, 30, 7, 5).toISOString())).toBe("07.05");
  });
  it("dayLabel", () => {
    expect(dayLabel("2026-10-30")).toBe("Jum, 30 Okt");
    expect(dayLabel(new Date(2026, 9, 29, 23, 30).toISOString())).toBe("Kam, 29 Okt");
  });
});

describe("format (en-US)", () => {
  it("usd and usd0 group with commas and point the decimals", () => {
    expect(usd(1234.56, "en")).toBe("$1,234.56");
    expect(usd(15.25, "en")).toBe("$15.25");
    expect(usd(null, "en")).toBe("–");
    expect(usd0(1240, "en")).toBe("$1,240");
  });
  it("idr keeps Rp, with en-US grouping and M/B units", () => {
    expect(idr(18.4, 16300, "en")).toBe("Rp299,920");
    expect(idr(68.2, 16300, "en")).toBe("Rp1.11M");
    expect(idr(1_240_000_000, 1, "en")).toBe("Rp1.24B");
    expect(idr(999_999.6, 1, "en")).toBe("Rp1M");
    expect(idr(999_999.4, 1, "en")).toBe("Rp999,999");
    expect(idr(0, 16300, "en")).toBe("Rp0");
  });
  it("tokens use K, M and B, picked after rounding", () => {
    expect(tokens(1_840_000, "en")).toBe("1.84M");
    expect(tokens(612_000, "en")).toBe("612K");
    expect(tokens(980, "en")).toBe("980");
    expect(tokens(999_500, "en")).toBe("1M");
    expect(tokens(999_499, "en")).toBe("999K");
    expect(tokens(5_200_000_000, "en")).toBe("5.2B");
    expect(tokens(999_995_000, "en")).toBe("1B");
  });
  it("dates read month first", () => {
    expect(shortDate("2026-10-31T08:00:00", "en")).toBe("Oct 31");
    expect(dayLabel("2026-10-30", "en")).toBe("Fri, Oct 30");
    expect(dayMonth("2026-10-31", "en")).toEqual({ day: "31", month: "Oct" });
    expect(shortDate("2026-10-31T08:00:00")).toBe("31 Okt");
    expect(dayMonth("2026-10-31")).toEqual({ day: "31", month: "Okt" });
  });
  it("time, duration, percent, multiple and counts", () => {
    expect(clock(new Date(2026, 9, 30, 12, 40).toISOString(), "en")).toBe("12:40");
    expect(clock(new Date(2026, 9, 30, 7, 5).toISOString(), "en")).toBe("07:05");
    expect(duration(38, "en")).toBe("38 min");
    expect(duration(72, "en")).toBe("1h 12m");
    expect(duration(120, "en")).toBe("2h");
    expect(pct(61.6, "en")).toBe("62%");
    expect(multiple(7.74, "en")).toBe("7.7×");
    expect(multiple(7.74)).toBe("7,7×");
    expect(price(1234.5, "en")).toBe("1,234.5");
    expect(price(1234.5)).toBe("1.234,5");
    expect(count(1200, "en")).toBe("1,200");
  });
});
