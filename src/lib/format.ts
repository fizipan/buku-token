import type { Lang } from "./api";

const DASH = "–";
const LOCALE: Record<Lang, string> = { id: "id-ID", en: "en-US" };

interface Formats {
  usd: Intl.NumberFormat;
  int: Intl.NumberFormat;
  mid: Intl.NumberFormat;
  clock: Intl.DateTimeFormat;
  day: Intl.DateTimeFormat;
  shortDay: Intl.DateTimeFormat;
}

const cache = new Map<Lang, Formats>();

/** Intl objects are costly to make: one set per language. */
function formats(lang: Lang): Formats {
  let f = cache.get(lang);
  if (!f) {
    const locale = LOCALE[lang];
    const num = (min: number, max: number) => new Intl.NumberFormat(locale, { minimumFractionDigits: min, maximumFractionDigits: max });
    f = {
      usd: num(2, 2),
      int: num(0, 0),
      mid: num(0, 2),
      clock: new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" }),
      day: new Intl.DateTimeFormat(locale, { weekday: "short", day: "numeric", month: "short" }),
      shortDay: new Intl.DateTimeFormat(locale, { day: "numeric", month: "short" }),
    };
    cache.set(lang, f);
  }
  return f;
}

/** "$15,25" (id) or "$15.25" (en); unknown cost (null) shows an en dash. */
export function usd(v: number | null, lang: Lang = "id"): string {
  return v === null ? DASH : `$${formats(lang).usd.format(v)}`;
}

/** Tokens were used but none has a known price: "$0,00" would be a lie. */
export const isUnpricedOnly = (costUsd: number, unpricedTokens: number): boolean => costUsd === 0 && unpricedTokens > 0;

/** "$1.240" (id) or "$1,240" (en) */
export function usd0(v: number | null, lang: Lang = "id"): string {
  return v === null ? DASH : `$${formats(lang).int.format(v)}`;
}

/** Rounds like Intl does for `digits` decimals, so unit choice can look at the rounded value. */
function round(v: number, digits: number, lang: Lang): number {
  const f = formats(lang);
  const text = (digits === 0 ? f.int : f.mid).format(v);
  // Drop the group separator, then turn the decimal separator into a point.
  return Number(lang === "id" ? text.replace(/\./g, "").replace(",", ".") : text.replace(/,/g, ""));
}

const UNITS: Record<Lang, { billion: string; million: string; thousand: string }> = {
  id: { billion: " M", million: " jt", thousand: " rb" },
  en: { billion: "B", million: "M", thousand: "K" },
};

/**
 * "Rp248.575"; from 1 jt "Rp20,21 jt"; from 1 M (miliar) "Rp1,24 M". English: "Rp248,575", "Rp20.21M",
 * "Rp1.24B". The unit is chosen after rounding.
 */
export function idr(usdValue: number, rate: number, lang: Lang = "id"): string {
  const rp = usdValue * rate;
  const { mid, int } = formats(lang);
  const u = UNITS[lang];
  if (round(rp / 1_000_000, 2, lang) >= 1000) return `Rp${mid.format(rp / 1e9)}${u.billion}`;
  if (round(rp, 0, lang) >= 1_000_000) return `Rp${mid.format(rp / 1e6)}${u.million}`;
  return `Rp${int.format(rp)}`;
}

/** "5,2 M", "1,24 jt", "612 rb", "980"; English "5.2B", "1.24M", "612K". The unit is chosen after rounding. */
export function tokens(n: number, lang: Lang = "id"): string {
  const { mid, int } = formats(lang);
  const u = UNITS[lang];
  if (round(n / 1e6, 2, lang) >= 1000) return `${mid.format(n / 1e9)}${u.billion}`;
  if (round(n / 1e3, 0, lang) >= 1000) return `${mid.format(n / 1e6)}${u.million}`;
  if (round(n, 0, lang) >= 1000) return `${int.format(n / 1e3)}${u.thousand}`;
  return int.format(n);
}

/** "38 mnt", "1j 12m", "2j"; English "38 min", "1h 12m", "2h". */
export function duration(min: number, lang: Lang = "id"): string {
  const [minute, hour] = lang === "id" ? ["mnt", "j"] : ["min", "h"];
  if (min < 60) return `${Math.round(min)} ${minute}`;
  const h = Math.floor(min / 60);
  const m = Math.round(min % 60);
  return m === 0 ? `${h}${hour}` : `${h}${hour} ${m}m`;
}

/** "62%" */
export function pct(v: number, lang: Lang = "id"): string {
  return `${formats(lang).int.format(v)}%`;
}

/** Local time, "12.40" (id) or "12:40" (en). */
export function clock(iso: string, lang: Lang = "id"): string {
  return formats(lang).clock.format(new Date(iso));
}

/** A bare "YYYY-MM-DD" is a local calendar day, not UTC midnight. */
const toDate = (iso: string): Date => {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso);
  return m ? new Date(+m[1], +m[2] - 1, +m[3]) : new Date(iso);
};

/** The day and the abbreviated month of a date: { day: "30", month: "Okt" }. */
export function dayMonth(iso: string, lang: Lang = "id"): { day: string; month: string } {
  const parts = formats(lang).shortDay.formatToParts(toDate(iso));
  const get = (t: string) => parts.find((p) => p.type === t)?.value ?? "";
  return { day: get("day"), month: get("month") };
}

/** "Jum, 30 Okt" (id) or "Fri, Oct 30" (en). */
export function dayLabel(iso: string, lang: Lang = "id"): string {
  const parts = formats(lang).day.formatToParts(toDate(iso));
  const get = (t: string) => parts.find((p) => p.type === t)?.value ?? "";
  return lang === "id" ? `${get("weekday")}, ${get("day")} ${get("month")}` : `${get("weekday")}, ${get("month")} ${get("day")}`;
}

/** Payback multiple with at most one decimal: 7.74 -> "7,7×" (id) or "7.7×" (en), 5 -> "5×". */
export function multiple(v: number, lang: Lang = "id"): string {
  return `${v.toLocaleString(LOCALE[lang], { maximumFractionDigits: 1 })}×`;
}

/** A price with at most two decimals: "1.234,5" (id) or "1,234.5" (en). */
export function price(v: number, lang: Lang = "id"): string {
  return v.toLocaleString(LOCALE[lang], { maximumFractionDigits: 2 });
}

/** A whole number with group separators. */
export function count(v: number, lang: Lang = "id"): string {
  return formats(lang).int.format(v);
}

/** "15 Okt" (id) or "Oct 15" (en). */
export function shortDate(iso: string, lang: Lang = "id"): string {
  return formats(lang).shortDay.format(new Date(iso));
}

/** The bound set of formatters for one language, what `useFmt` hands out. */
export interface Fmt {
  lang: Lang;
  usd: (v: number | null) => string;
  usd0: (v: number | null) => string;
  idr: (usdValue: number, rate: number) => string;
  tokens: (n: number) => string;
  duration: (min: number) => string;
  pct: (v: number) => string;
  clock: (iso: string) => string;
  dayLabel: (iso: string) => string;
  dayMonth: (iso: string) => { day: string; month: string };
  multiple: (v: number) => string;
  price: (v: number) => string;
  count: (v: number) => string;
  shortDate: (iso: string) => string;
}

const bound = new Map<Lang, Fmt>();

export function formattersFor(lang: Lang): Fmt {
  let f = bound.get(lang);
  if (!f) {
    f = {
      lang,
      usd: (v) => usd(v, lang),
      usd0: (v) => usd0(v, lang),
      idr: (v, rate) => idr(v, rate, lang),
      tokens: (n) => tokens(n, lang),
      duration: (m) => duration(m, lang),
      pct: (v) => pct(v, lang),
      clock: (iso) => clock(iso, lang),
      dayLabel: (iso) => dayLabel(iso, lang),
      dayMonth: (iso) => dayMonth(iso, lang),
      multiple: (v) => multiple(v, lang),
      price: (v) => price(v, lang),
      count: (v) => count(v, lang),
      shortDate: (iso) => shortDate(iso, lang),
    };
    bound.set(lang, f);
  }
  return f;
}
