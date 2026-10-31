import { ChevronDown, ChevronUp, Info, Share2, ShieldCheck } from "lucide-react";
import { useLayoutEffect, useRef, useState, type RefObject } from "react";
import { AgentDonut } from "@/charts/AgentDonut";
import { BurnChart } from "@/charts/BurnChart";
import { StackedColumns } from "@/charts/StackedColumns";
import { AgentTile } from "@/components/AgentTile";
import { DataView } from "@/components/DataView";
import { countdown, QuotaRow } from "@/components/QuotaRow";
import { SharePanel } from "@/components/SharePanel";
import { Unpriced } from "@/components/Unpriced";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useTabData } from "@/hooks/useTabData";
import { useDisplayPrefs, useUsdToIdr } from "@/hooks/useUsdToIdr";
import { AGENT_NAME, STACK_ORDER } from "@/lib/agents";
import {
  getOverview,
  openSettings,
  refresh,
  type AgentKey,
  type AgentQuota,
  type Overview,
} from "@/lib/api";
import { isUnpricedOnly } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { isExpiredWindow, isStaleWindow, weeklyPace } from "@/lib/quota";
import { AntigravityRow, OpenRouterSection } from "./ServiceSections";

/** Agents with a quota get a section, in this order. */
const SECTION_ORDER: readonly AgentKey[] = ["cc", "cx", "cr"];
const HEAD_CLASS = "flex w-full min-w-0 items-center gap-[9px]";
const STORAGE_KEY = "buku-token:ringkasan-open";

/** `or` is the OpenRouter section; it is not an agent. */
type OpenMap = Partial<Record<AgentKey | "or", boolean>>;

function readSaved(): OpenMap {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}");
    if (!raw || typeof raw !== "object") return {};
    return Object.fromEntries(
      Object.entries(raw).filter(([, v]) => typeof v === "boolean"),
    );
  } catch {
    return {};
  }
}

function writeSaved(open: OpenMap) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(open));
  } catch {
    // Storage blocked or full: the choice just lasts for this session.
  }
}

export function Ringkasan() {
  const state = useTabData(getOverview, []);
  const rate = useUsdToIdr();
  return (
    <TooltipProvider>
      <DataView state={state}>
        {(overview) => <Body o={overview} rate={rate} />}
      </DataView>
    </TooltipProvider>
  );
}

function Body({ o, rate }: { o: Overview; rate: number }) {
  const t = useI18n().t;
  const now = new Date();
  const [saved, setSaved] = useState(readSaved);
  const [chosen, setChosen] = useState<OpenMap>({});

  const sections = SECTION_ORDER.flatMap((agent) => {
    const quota = o.quotas.find((q) => q.agent === agent);
    return quota ? [{ agent, quota }] : [];
  });
  const firstFolding = sections.find((s) => s.quota.windows.length > 0)?.agent;

  // This session's choice, then a quota close to its limit, then the saved choice, then the default.
  const isOpen = (agent: AgentKey) =>
    chosen[agent] ??
    (!!o.quotas.find((q) => q.agent === agent)?.nearLimit ||
      (saved[agent] ?? agent === firstFolding));
  const remember = (key: keyof OpenMap, next: boolean) => {
    const all = { ...saved, [key]: next };
    setChosen((c) => ({ ...c, [key]: next }));
    setSaved(all);
    writeSaved(all);
  };
  const toggle = (agent: AgentKey) => remember(agent, !isOpen(agent));
  const orOpen = chosen.or ?? saved.or ?? false;

  // The panel takes the tab's place; the tab stays mounted (hidden), so its choices and data survive.
  const [sharing, setSharing] = useState(false);
  const shareButton = useRef<HTMLButtonElement>(null);
  const returnFocus = useRef(false);
  const closeShare = () => {
    returnFocus.current = true;
    setSharing(false);
  };
  useLayoutEffect(() => {
    if (!sharing && returnFocus.current) {
      returnFocus.current = false;
      shareButton.current?.focus();
    }
  }, [sharing]);

  const burnFive = o.quotas
    .find((q) => q.agent === "cc")
    ?.windows.find((w) => w.id === "five_hour");
  const showBurn = !!o.burn && !!burnFive && !isExpiredWindow(burnFive, now);

  return (
    <>
      {sharing && <SharePanel onClose={closeShare} />}
      <div data-testid="ringkasan-body" hidden={sharing}>
        <Summary today={o.today} rate={rate} onShare={() => setSharing(true)} shareRef={shareButton} />
        <div className="divide-y border-t">
          {sections.map(({ agent, quota }) => (
            <AgentSection
              key={agent}
              agent={agent}
              quota={quota}
              open={isOpen(agent)}
              onToggle={() => toggle(agent)}
              burn={agent === "cc" && showBurn ? o.burn : null}
              now={now}
            />
          ))}
          {o.openrouter && <OpenRouterSection view={o.openrouter} open={orOpen} onToggle={() => remember("or", !orOpen)} />}
          {o.antigravity && <AntigravityRow activity={o.antigravity} />}

          <section className="px-5 pb-1 pt-5">
            <h2 className="mb-3 text-[15px] font-semibold">{t("hoursHeading")}</h2>
            <Hours today={o.today} now={now} />
          </section>
        </div>

        {o.rtk && <RtkRow rtk={o.rtk} />}
        <div className="h-5" />
      </div>
    </>
  );
}

/** Donut of today's cost per agent, the list beside it, and the Rupiah and token totals. */
function Summary({
  today,
  rate,
  onShare,
  shareRef,
}: {
  today: Overview["today"];
  rate: number;
  onShare: () => void;
  shareRef: RefObject<HTMLButtonElement | null>;
}) {
  const { t, f } = useI18n();
  const noPrice = isUnpricedOnly(today.costUsd, today.unpricedTokens);
  // Highest cost first, agents with no known price after the priced ones; ties keep STACK_ORDER.
  const items = STACK_ORDER.flatMap((agent, i) =>
    today.byAgent[i] > 0 || today.unpricedByAgent[i] > 0
      ? [{ agent, cost: today.byAgent[i], unpriced: today.unpricedByAgent[i] }]
      : [],
  ).sort((a, b) => b.cost - a.cost || b.unpriced - a.unpriced);
  const total = noPrice ? "–" : f.usd(today.costUsd);
  return (
    <section className="relative px-5 py-[13px]">
      <Button
        ref={shareRef}
        variant="ghost"
        size="icon-xs"
        aria-label={t("shareUsage")}
        className="absolute right-3 top-2 text-muted-foreground"
        onClick={onShare}
      >
        <Share2 aria-hidden />
      </Button>
      <div className="flex items-center gap-5">
        <AgentDonut
          byAgent={today.byAgent}
          centre={total}
          aria-label={t("donutAria", { total: noPrice ? t("noPrice") : total })}
        />
        {items.length > 0 && (
          <ul
            data-testid="summary-legend"
            className="grid min-w-0 flex-1 gap-[7px] pt-[22px] text-[13.5px]"
          >
            {items.map(({ agent, cost, unpriced }) => (
              <li key={agent} className="flex min-w-0 items-center gap-[9px]">
                <i
                  aria-hidden
                  className="size-[9px] shrink-0 rounded-full"
                  style={{ background: `var(--a-${agent})` }}
                />
                <span className="min-w-0 flex-1 truncate">{AGENT_NAME[agent]}</span>
                <b className="whitespace-nowrap font-medium tabular-nums text-muted-foreground">
                  {isUnpricedOnly(cost, unpriced) ? <Unpriced /> : f.usd(cost)}
                </b>
              </li>
            ))}
          </ul>
        )}
      </div>
      <dl
        data-testid="summary-stats"
        className="mt-3.5 grid min-w-0 grid-cols-2 rounded-[10px] bg-muted"
      >
        <div className="min-w-0 px-3.5 py-[9px]">
          <dt className="text-xs text-muted-foreground">{t("inRupiah")}</dt>
          <dd className="mt-0.5 whitespace-nowrap text-[15px] font-semibold tracking-[-0.01em] tabular-nums">
            {noPrice ? "–" : f.idr(today.costUsd, rate)}
          </dd>
        </div>
        <div className="min-w-0 border-l px-3.5 py-[9px]">
          <dt className="text-xs text-muted-foreground">{t("totalTokens")}</dt>
          <dd className="mt-0.5 whitespace-nowrap text-[15px] font-semibold tracking-[-0.01em] tabular-nums">
            {f.tokens(today.tokens)}
          </dd>
        </div>
      </dl>
    </section>
  );
}

interface AgentSectionProps {
  agent: AgentKey;
  quota: AgentQuota;
  open: boolean;
  onToggle: () => void;
  burn: Overview["burn"];
  now: Date;
}

/** The one reset time every window shares (to the minute), or null. Needs two or more live windows. */
function sharedReset(windows: AgentQuota["windows"], now: Date): string | null {
  if (windows.length < 2) return null;
  if (windows.some((w) => isExpiredWindow(w, now) || isStaleWindow(w, now))) return null;
  const minute = (w: { resetsAt: string }) => Math.floor(new Date(w.resetsAt).getTime() / 60_000);
  return windows.every((w) => minute(w) === minute(windows[0])) ? windows[0].resetsAt : null;
}

function AgentHead({
  agent,
  badge,
  note = null,
  chevron = null,
}: {
  agent: AgentKey;
  badge: string | null;
  /** Small muted text before the chevron, e.g. "Reset 7 Nov". */
  note?: string | null;
  chevron?: "up" | "down" | null;
}) {
  const Chevron = chevron === "up" ? ChevronUp : ChevronDown;
  return (
    <>
      <AgentTile agent={agent} size={26} />
      <span className="truncate text-[14.5px] font-semibold tracking-[-0.01em]">
        {AGENT_NAME[agent]}
      </span>
      {badge && <Badge variant="secondary">{badge}</Badge>}
      {(note || chevron) && (
        <span className="ml-auto flex shrink-0 items-center gap-1.5">
          {note && (
            <span className="whitespace-nowrap text-xs text-muted-foreground">{note}</span>
          )}
          {chevron && (
            <Chevron aria-hidden className="size-4 shrink-0 text-muted-foreground" />
          )}
        </span>
      )}
    </>
  );
}

function AgentSection({ agent, quota, open, onToggle, burn, now }: AgentSectionProps) {
  const prefs = useDisplayPrefs();
  const { t, f } = useI18n();
  const sectionProps = {
    "data-testid": "agent-section",
    "data-agent": agent,
    className: "min-w-0 px-5 py-[13px]",
  };

  const { windows } = quota;
  if (windows.length === 0) {
    return (
      <section {...sectionProps}>
        <div className={HEAD_CLASS}>
          <AgentHead agent={agent} badge={quota.plan} />
        </div>
        {quota.unavailableReason && (
          <div
            data-testid="quota-reason-row"
            className="mt-[11px] flex items-center justify-between gap-3 text-[12.5px]"
          >
            <span className="min-w-0 text-muted-foreground">
              {quota.unavailableReason}
            </span>
            {agent === "cr" ? (
              <Button
                variant="outline"
                size="sm"
                className="shrink-0"
                onClick={() => void openSettings().catch(() => {})}
              >
                {t("openSettings")}
              </Button>
            ) : (
              <Button
                variant="outline"
                size="sm"
                className="shrink-0"
                onClick={() => void refresh().catch(() => {})}
              >
                {t("retry")}
              </Button>
            )}
          </div>
        )}
      </section>
    );
  }

  const reset = sharedReset(windows, now);
  const shown = open ? windows : [quota.binding ?? windows[0]];
  const rowsId = `ringkasan-rows-${agent}`;
  return (
    <section {...sectionProps}>
      <button
        type="button"
        aria-expanded={open}
        aria-controls={rowsId}
        onClick={onToggle}
        className={`${HEAD_CLASS} rounded-md text-left outline-none focus-visible:ring-[3px] focus-visible:ring-ring`}
      >
        <AgentHead
          agent={agent}
          badge={quota.plan}
          note={reset ? (prefs.resetDisplay === "countdown" ? t("resetIn", { time: countdown(reset, now, t, f.duration) }) : t("resetOn", { date: f.shortDate(reset) })) : null}
          chevron={open ? "up" : "down"}
        />
      </button>
      <div id={rowsId} className="mt-[11px] grid grid-cols-1 gap-[9px]">
        {shown.map((w) => (
          <QuotaRow
            key={w.id}
            agent={agent}
            window={w}
            pace={
              w.id.startsWith("seven_day") && !isStaleWindow(w, now)
                ? weeklyPace(w.resetsAt, now)
                : null
            }
            now={now}
            hideReset={reset !== null}
            quotaDisplay={prefs.quotaDisplay}
            resetDisplay={prefs.resetDisplay}
          />
        ))}
      </div>
      {open && burn && (
        <div className="mt-2.5">
          <BurnChart burn={burn} height={128} color={`var(--a-${agent})`} />
        </div>
      )}
    </section>
  );
}

function Hours({ today, now }: { today: Overview["today"]; now: Date }) {
  const { t, f } = useI18n();
  const empty =
    today.costUsd === 0 &&
    today.hours.every((h) => h.byAgent.every((v) => v === 0));
  if (empty) {
    return (
      <p className="py-6 text-center text-[13px] text-muted-foreground">
        {today.unpricedTokens > 0 ? t("noPriceToday") : t("noSessionsToday")}
      </p>
    );
  }
  const hh = (n: number) => String(n).padStart(2, "0");
  const current = today.hours.findIndex((h) => h.hour === now.getHours());
  return (
    <StackedColumns
      data={today.hours.map((h) => ({ label: hh(h.hour), byAgent: h.byAgent }))}
      height={120}
      showLegend={false}
      highlightIndex={current >= 0 ? current : undefined}
      valueFormat={f.usd0}
      tooltipFormat={f.usd}
      tooltipTitle={(l) => t("hourRange", { h: l })}
      aria-label={t("hoursAria")}
    />
  );
}

function RtkRow({ rtk }: { rtk: NonNullable<Overview["rtk"]> }) {
  const { t, f } = useI18n();
  return (
    <div className="mx-5 mt-4 flex items-center justify-between gap-3 border-t pt-3.5 text-[13px]">
      <span className="flex items-center gap-2 text-muted-foreground">
        <ShieldCheck aria-hidden className="size-4" />
        {t("rtkSaved")}
      </span>
      <span className="flex items-center gap-1 tabular-nums">
        <b className="font-semibold">{t("tokenCount", { n: f.tokens(rtk.savedTokens) })}</b>
        {rtk.approxUsd !== null && (
          <>
            <span aria-hidden>·</span>
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  className="inline-flex items-center gap-1 rounded-sm outline-none focus-visible:ring-[3px] focus-visible:ring-ring"
                >
                  ≈ {f.usd(rtk.approxUsd)}
                  <Info aria-hidden className="size-3 text-muted-foreground" />
                </button>
              </TooltipTrigger>
              <TooltipContent side="top">
                {t("rtkTip", { model: rtk.model ?? "model" })}
              </TooltipContent>
            </Tooltip>
          </>
        )}
      </span>
    </div>
  );
}
