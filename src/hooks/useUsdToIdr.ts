import { useSyncExternalStore } from "react";
import { getSettings, onDataUpdated, type AgentKey, type Lang, type QuotaDisplay, type ResetDisplay, type Settings, type ThemePref } from "../lib/api";

const FALLBACK = 16300;

export interface DisplayPrefs {
  theme: ThemePref;
  quotaDisplay: QuotaDisplay;
  resetDisplay: ResetDisplay;
  language: Lang;
  /** Agents switched off in Pengaturan are `false`; a missing one counts as on. */
  enabledAgents: Partial<Record<AgentKey, boolean>>;
}

const DEFAULT_PREFS: DisplayPrefs = { theme: "system", quotaDisplay: "left", resetDisplay: "clock", language: "id", enabledAgents: {} };

let rate = FALLBACK;
let prefs = DEFAULT_PREFS;
let loaded = false;
let pending: Promise<void> | null = null;
let listening = false;
const subs = new Set<() => void>();

/** Stores what the popover needs from the settings. Subscribers hear about a change only when something differs. */
function apply(s: Settings) {
  const next: DisplayPrefs = {
    theme: s.theme ?? DEFAULT_PREFS.theme,
    quotaDisplay: s.quotaDisplay ?? DEFAULT_PREFS.quotaDisplay,
    resetDisplay: s.resetDisplay ?? DEFAULT_PREFS.resetDisplay,
    language: s.language ?? DEFAULT_PREFS.language,
    enabledAgents: s.enabledAgents ?? DEFAULT_PREFS.enabledAgents,
  };
  const same =
    next.theme === prefs.theme &&
    next.quotaDisplay === prefs.quotaDisplay &&
    next.resetDisplay === prefs.resetDisplay &&
    next.language === prefs.language &&
    JSON.stringify(next.enabledAgents) === JSON.stringify(prefs.enabledAgents);
  if (s.usdToIdr === rate && same) return;
  rate = s.usdToIdr;
  if (!same) prefs = next;
  subs.forEach((f) => f());
}

function load() {
  if (loaded || pending) return;
  pending = getSettings().then(
    (s) => {
      loaded = true;
      apply(s);
    },
    () => {
      pending = null; // keep the fallback; the next mount retries
    },
  );
}

/** Refetch after the Settings window (or anything else) changed data. Keeps the old values on failure. */
function refetch() {
  getSettings().then(apply, () => {});
}

function listen() {
  if (listening) return;
  listening = true;
  void onDataUpdated(() => (loaded ? refetch() : load())).catch(() => {
    listening = false;
  });
}

function subscribe(cb: () => void) {
  subs.add(cb);
  load();
  listen();
  return () => void subs.delete(cb);
}

/** USD to IDR rate from settings. Loaded once per app run and shared by every tab; 16300 until then. */
export function useUsdToIdr(): number {
  return useSyncExternalStore(subscribe, () => rate);
}

/** Theme, language, Sisa/Terpakai and the reset format, from the same shared load as the rate. */
export function useDisplayPrefs(): DisplayPrefs {
  return useSyncExternalStore(subscribe, () => prefs);
}

/** Test helper. */
export function resetUsdToIdrCache() {
  rate = FALLBACK;
  prefs = DEFAULT_PREFS;
  loaded = false;
  pending = null;
  listening = false;
  subs.clear();
}
