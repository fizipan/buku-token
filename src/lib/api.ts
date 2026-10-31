import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Mirrors the Rust DTOs (serde camelCase). Dates are RFC3339 strings, calendar days "YYYY-MM-DD".
// `byAgent` arrays follow STACK_ORDER [cc, cx, oc, hm, cr]. Shares are percents 0-100. Unknown cost = null.

export type AgentKey = "cc" | "cx" | "oc" | "hm" | "cr";
export type Period = "today" | "7d" | "14d" | "30d";

export interface ProjectRef {
  /** "" = no project (named "Tanpa Project" or "No Project" by the backend) */
  path: string;
  name: string;
  displayPath: string;
}

export interface QuotaWindow {
  /** "five_hour" | "seven_day" | "seven_day_opus" ... */
  id: string;
  label: string;
  usedPct: number;
  resetsAt: string;
  observedAt: string;
  expired: boolean;
}

export interface AgentQuota {
  agent: AgentKey;
  plan: string | null;
  windows: QuotaWindow[];
  binding: QuotaWindow | null;
  nearLimit: boolean;
  unavailableReason: string | null;
}

export interface Burn {
  windowStart: string;
  resetsAt: string;
  /** [minutes since windowStart, usedPct] */
  points: [number, number][];
  projectedAtReset: number;
  exhaustsAt: string | null;
}

export interface HourBucket {
  /** Local hour 6..=23 */
  hour: number;
  byAgent: [number, number, number, number, number];
}

export interface TodaySummary {
  costUsd: number;
  unpricedTokens: number;
  tokens: number;
  /** USD per agent in `STACK_ORDER`, whole day */
  byAgent: [number, number, number, number, number];
  /** Tokens without a known price per agent in `STACK_ORDER` */
  unpricedByAgent: [number, number, number, number, number];
  /** Always 18 buckets (hours 6..=23) */
  hours: HourBucket[];
}

export interface RtkSummary {
  savedTokens: number;
  approxUsd: number | null;
  model: string | null;
}

export interface KeyLimit {
  limitUsd: number;
  remainingUsd: number;
  reset: "daily" | "weekly" | "monthly" | null;
}

export interface OpenRouterData {
  /** Free tier: shows "Free" instead of "Per Pakai" */
  free: boolean;
  /** null when this key may not read /credits */
  balanceUsd: number | null;
  todayUsd: number;
  weekUsd: number;
  monthUsd: number;
  limit: KeyLimit | null;
}

/** OpenRouter is not an agent: it is in none of the `byAgent` arrays, the donut or the totals. */
export interface OpenRouterView {
  data: OpenRouterData | null;
  /** Fixed text, e.g. "Sedang Offline" */
  unavailableReason: string | null;
}

/** Antigravity activity from file dates only. No tokens, cost or quota. */
export interface AntigravityActivity {
  today: number;
  last7Days: number;
  /** Latest real change time of any session; null when no session has a date */
  lastActive: string | null;
  totalSessions: number;
  totalSteps: number;
}

export interface Overview {
  burn: Burn | null;
  quotas: AgentQuota[];
  today: TodaySummary;
  rtk: RtkSummary | null;
  /** null = not connected, switched off, or no key */
  openrouter: OpenRouterView | null;
  /** null = switched off, or no session found at all (old sessions still show) */
  antigravity: AntigravityActivity | null;
  updatedAt: string;
}

export interface SessionFilter {
  agent?: AgentKey | null;
  /** ProjectRef.path */
  projectPath?: string | null;
  /** Only sessions starting before this local day "YYYY-MM-DD" (next page) */
  before?: string | null;
  /** Number of days with sessions to return (1..=60) */
  days: number;
}

export interface SessionRow {
  sessionId: string;
  agent: AgentKey;
  project: ProjectRef;
  startedAt: string;
  durationMin: number;
  mainModel: string;
  tokens: number;
  costUsd: number | null;
  unpricedTokens: number;
}

export interface DayGroup {
  date: string;
  /** "Hari Ini · Jum, 30 Okt" or "Today · Fri, Oct 30", made by the backend in the current language */
  label: string;
  /** Sum of known prices */
  costUsd: number;
  /** Tokens of this day's sessions without a price */
  unpricedTokens: number;
  sessions: SessionRow[];
}

export interface SessionPage {
  days: DayGroup[];
  hasMore: boolean;
  /** Options for the project Select (ignores the filter) */
  projects: ProjectRef[];
}

export interface ModelRow {
  model: string;
  agent: AgentKey;
  costUsd: number | null;
  unpricedTokens: number;
  tokens: number;
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  share: number | null;
}

/** Percent shares per token kind. */
export interface Mix {
  cacheRead: number;
  input: number;
  output: number;
  cacheWrite: number;
}

export interface CacheInsight {
  tokenShare: number;
  costShare: number;
  costWithoutCache: number;
}

export interface ModelsView {
  rows: ModelRow[];
  totalCostUsd: number;
  totalTokens: number;
  unpricedTokens: number;
  tokenMix: { tokens: Mix; cost: Mix };
  cacheInsight: CacheInsight | null;
}

export interface ProjectRow {
  project: ProjectRef;
  byAgent: [number, number, number, number, number];
  costUsd: number;
  unpricedTokens: number;
  sessions: number;
  share: number;
  /** 0 for a real project; N for the "N Project Lainnya" row */
  folded: number;
}

export interface ProjectInsight {
  project: ProjectRef;
  share: number;
  prevShare: number;
}

export interface ProjectsView {
  rows: ProjectRow[];
  totalCostUsd: number;
  insight: ProjectInsight | null;
}

export interface DayCost {
  date: string;
  byAgent: [number, number, number, number, number];
  /** Tokens of this day without a price */
  unpricedTokens: number;
}

export interface Cycle {
  start: string;
  endExclusive: string;
  dayOfCycle: number;
  length: number;
}

export interface PlanValue {
  planId: string;
  name: string;
  agents: AgentKey[];
  priceUsd: number;
  cycle: Cycle;
  apiEquivalentUsd: number;
  multiple: number;
  projectedUsd: number;
  projectedMultiple: number;
}

export interface HistoryView {
  days: DayCost[];
  totalUsd: number;
  avgPerDay: number;
  unpricedTokens: number;
  plans: PlanValue[];
}

export interface Plan {
  id: string;
  name: string;
  priceUsd: number;
  renewalDay: number;
  agents: AgentKey[];
}

/** The language of every text in the app. */
export type Lang = "id" | "en";
export type ThemePref = "system" | "light" | "dark";
export type QuotaDisplay = "left" | "used";
export type ResetDisplay = "countdown" | "clock";

export interface Settings {
  usdToIdr: number;
  plans: Plan[];
  /** A missing key counts as enabled */
  enabledAgents: Partial<Record<AgentKey, boolean>>;
  launchAtLogin: boolean;
  lastTab: string;
  onboarded: boolean;
  theme: ThemePref;
  quotaDisplay: QuotaDisplay;
  resetDisplay: ResetDisplay;
  notifyLowQuota: boolean;
  notifyProjectedExhaustion: boolean;
  hideOnScreenShare: boolean;
  openrouterEnabled: boolean;
  antigravityEnabled: boolean;
  language: Lang;
}

export interface SourceStatus {
  agent: AgentKey;
  path: string;
  found: boolean;
}

/** A folder read for activity only (not an agent). */
export interface ActivitySource {
  name: string;
  path: string;
  found: boolean;
}

export interface ScanStatus {
  running: boolean;
  done: number;
  total: number;
  skipped: number;
  errors: number;
  lastScanAt: string | null;
  pricingVersion: string;
  sources: SourceStatus[];
  activitySources: ActivitySource[];
}

export interface CursorSessionStatus {
  connected: boolean;
  /** Known only when the cookie is a readable JWT. */
  expiresAt: string | null;
  /** The Keychain refused Buku Token's own item; it is not read again until Hubungkan or Putuskan. */
  keychainDenied: boolean;
  /** Why the last quota poll failed (fixed text), shown next to "Terhubung". */
  lastReason: string | null;
  historyFailed: boolean;
}

export interface OpenRouterSessionStatus {
  connected: boolean;
  /** The Keychain refused Buku Token's own item; it is not read again until Hubungkan or Putuskan. */
  keychainDenied: boolean;
  /** Why the last poll failed (fixed text), shown next to "Akun Terhubung". */
  lastReason: string | null;
}

/** What the share image shows. The image holds no project name, path or conversation text. */
export interface ShareRequest {
  period: Period;
  /** null = every agent that is switched on */
  agent: AgentKey | null;
  /** No money in the image: shares and the centre are of tokens */
  hideCost: boolean;
  dark: boolean;
}

export interface ScanProgress {
  done: number;
  total: number;
}

// Commands. Errors reject with the string from Rust.
export const getOverview = () => invoke<Overview>("get_overview");
export const getSessions = (filter: SessionFilter) => invoke<SessionPage>("get_sessions", { filter });
export const getModels = (period: Period) => invoke<ModelsView>("get_models", { period });
export const getProjects = (period: Period) => invoke<ProjectsView>("get_projects", { period });
export const getHistory = (days: number) => invoke<HistoryView>("get_history", { days });
/** Resolves after a scan pass and a forced Claude quota poll. */
export const refresh = () => invoke<null>("refresh");
export const getSettings = () => invoke<Settings>("get_settings");
/** Partial: given top-level fields replace the stored ones. */
export const saveSettings = (settings: Partial<Settings>) => invoke<Settings>("save_settings", { settings });
/** Resolves to the new pricing version "YYYY-MM-DD". */
export const updatePricing = () => invoke<string>("update_pricing");
/** Sends one sample notification. It cannot tell whether macOS showed it. */
export const sendTestNotification = () => invoke<null>("send_test_notification");
export const resizePopover = (height: number) => invoke<null>("resize_popover", { height });
export const openSettings = () => invoke<null>("open_settings");
export const scanStatus = () => invoke<ScanStatus>("scan_status");
// The cookie travels only in connect's argument; no call ever returns it.
export const cursorSessionStatus = () => invoke<CursorSessionStatus>("cursor_session_status");
/** The last Cursor failure in the current language, from memory (no Keychain read). */
export const cursorLastReason = () => invoke<string | null>("cursor_last_reason");
export const connectCursor = (cookie: string) => invoke<CursorSessionStatus>("connect_cursor", { cookie });
export const disconnectCursor = () => invoke<null>("disconnect_cursor");
// The key travels only in connect's argument; no call ever returns it.
export const openrouterSessionStatus = () => invoke<OpenRouterSessionStatus>("openrouter_session_status");
export const connectOpenRouter = (key: string) => invoke<OpenRouterSessionStatus>("connect_openrouter", { key });
export const disconnectOpenRouter = () => invoke<null>("disconnect_openrouter");
// Share image: all local. Errors are fixed texts without paths.
/** The card as a `data:image/png` URL. */
export const sharePreview = (request: ShareRequest) => invoke<string>("share_preview", { request });
export const shareCopy = (request: ShareRequest) => invoke<null>("share_copy", { request });
/** Resolves to the file name written in Downloads. */
export const shareSave = (request: ShareRequest) => invoke<string>("share_save", { request });

// Events. Each returns the unlisten function.
export const onScanProgress = (cb: (p: ScanProgress) => void): Promise<UnlistenFn> =>
  listen<ScanProgress>("scan-progress", (e) => cb(e.payload));
export const onDataUpdated = (cb: () => void): Promise<UnlistenFn> => listen("data-updated", () => cb());
