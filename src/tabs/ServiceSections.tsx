import { ChevronDown, ChevronUp, Info } from "lucide-react";
import type { CSSProperties } from "react";
import { ServiceTile } from "@/components/ServiceTile";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { openSettings, type AntigravityActivity, type KeyLimit, type OpenRouterData, type OpenRouterView } from "@/lib/api";
import { useI18n, type Fmt, type Key, type T } from "@/lib/i18n";

/** OpenRouter and Antigravity sit beside the agents, not in them: nothing here feeds the donut or the totals. */
const RESET: Record<NonNullable<KeyLimit["reset"]>, Key> = {
  daily: "resetDaily",
  weekly: "resetWeekly",
  monthly: "resetMonthly",
};
/** Remaining share of a key limit at or below which the figure turns red, like a quota. */
const LOW_LEFT = 15;
const NEUTRAL = "var(--muted-foreground)";
const TRACK = { "--bar": NEUTRAL, background: `color-mix(in oklab, ${NEUTRAL} 16%, var(--background))` } as CSSProperties;
const HEAD = "flex w-full min-w-0 items-center gap-[9px]";

function Head({ badge, chevron }: { badge: string | null; chevron?: "up" | "down" }) {
  const Chevron = chevron === "up" ? ChevronUp : ChevronDown;
  return (
    <>
      <ServiceTile service="openrouter" size={26} />
      <span className="truncate text-[14.5px] font-semibold tracking-[-0.01em]">OpenRouter</span>
      {badge && <Badge variant="secondary">{badge}</Badge>}
      {chevron && <Chevron aria-hidden className="ml-auto size-4 shrink-0 text-muted-foreground" />}
    </>
  );
}

function Line({ label, value }: { label: string; value: string }) {
  return (
    <div data-testid="openrouter-row" className="flex items-baseline justify-between gap-3 text-[12.5px]">
      <span className="text-muted-foreground">{label}</span>
      <span className="whitespace-nowrap font-semibold tabular-nums">{value}</span>
    </div>
  );
}

function LimitLine({ limit }: { limit: KeyLimit }) {
  const { t, f } = useI18n();
  const left = limit.limitUsd > 0 ? Math.min(100, (limit.remainingUsd / limit.limitUsd) * 100) : 0;
  return (
    <div data-testid="openrouter-limit" className="grid min-w-0 grid-cols-[92px_1fr_auto] items-center gap-x-2.5 text-[12.5px]">
      <span className="truncate text-muted-foreground">{t("limitLeft")}</span>
      <Progress
        value={Math.round(left)}
        aria-label={t("limitLeft")}
        className="h-1.5 [&_[data-slot=progress-indicator]]:rounded-full [&_[data-slot=progress-indicator]]:bg-(--bar)"
        style={TRACK}
      />
      <span className={`whitespace-nowrap text-right font-semibold tabular-nums ${left <= LOW_LEFT ? "text-destructive" : ""}`}>
        {t("amountOf", { a: f.usd(limit.remainingUsd), b: f.usd(limit.limitUsd) })}
      </span>
      {limit.reset && <p className="col-span-full mt-1 text-xs text-muted-foreground">{t(RESET[limit.reset])}</p>}
    </div>
  );
}

function Numbers({ d, open }: { d: OpenRouterData; open: boolean }) {
  const { t, f } = useI18n();
  const balance = d.balanceUsd === null ? [] : [[t("balance"), f.usd(d.balanceUsd)] as const];
  const all = [...balance, [t("today"), f.usd(d.todayUsd)] as const, [t("thisWeek"), f.usd(d.weekUsd)] as const, [t("thisMonth"), f.usd(d.monthUsd)] as const];
  // Closed: the balance when the key may read it, else this month.
  const shown = open ? all : [balance[0] ?? all[all.length - 1]];
  return (
    <div className="mt-[11px] grid grid-cols-1 gap-[9px]">
      {open && <p className="text-xs text-muted-foreground">{t("keyUsage")}</p>}
      {open && balance.length === 0 && <p className="text-xs text-muted-foreground">{t("noBalance")}</p>}
      {shown.map(([label, value]) => (
        <Line key={label} label={label} value={value} />
      ))}
      {open && d.limit && <LimitLine limit={d.limit} />}
    </div>
  );
}

export function OpenRouterSection({ view, open, onToggle }: { view: OpenRouterView; open: boolean; onToggle: () => void }) {
  const t = useI18n().t;
  const { data } = view;
  const props = { "data-testid": "openrouter-section", className: "min-w-0 px-5 py-[13px]" };
  if (!data) {
    return (
      <section {...props}>
        <div className={HEAD}>
          <Head badge={null} />
        </div>
        {view.unavailableReason && (
          <div data-testid="openrouter-reason-row" className="mt-[11px] flex items-center justify-between gap-3 text-[12.5px]">
            <span className="min-w-0 text-muted-foreground">{view.unavailableReason}</span>
            <Button variant="outline" size="sm" className="shrink-0" onClick={() => void openSettings().catch(() => {})}>
              {t("openSettings")}
            </Button>
          </div>
        )}
      </section>
    );
  }
  return (
    <section {...props}>
      <button
        type="button"
        aria-expanded={open}
        aria-controls="ringkasan-rows-openrouter"
        onClick={onToggle}
        className={`${HEAD} rounded-md text-left outline-none focus-visible:ring-[3px] focus-visible:ring-ring`}
      >
        <Head badge={data.free ? t("free") : t("payAsYouGo")} chevron={open ? "up" : "down"} />
      </button>
      <div id="ringkasan-rows-openrouter">
        <Numbers d={data} open={open} />
      </div>
      <p data-testid="openrouter-note" className="mt-2.5 flex items-center gap-1 text-xs text-muted-foreground">
        {t("notAddedToTotal")}
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              aria-label={t("whyNotAdded")}
              className="inline-flex rounded-sm outline-none focus-visible:ring-[3px] focus-visible:ring-ring"
            >
              <Info aria-hidden className="size-3" />
            </button>
          </TooltipTrigger>
          <TooltipContent side="top" className="max-w-[260px]">
            {t("notAddedWhy")}
          </TooltipContent>
        </Tooltip>
      </p>
    </section>
  );
}

/** Recent activity, or (when nothing changed in 7 days) the totals and when the last session was. */
function antigravityText(a: AntigravityActivity, t: T, f: Fmt): string {
  if (a.last7Days > 0) return t("antigravityRecent", { today: a.today, week: a.last7Days });
  return [
    t("antigravitySessions", { n: f.count(a.totalSessions) }),
    a.totalSteps > 0 && t("antigravitySteps", { n: f.count(a.totalSteps) }),
    a.lastActive && t("antigravityLast", { date: f.shortDate(a.lastActive) }),
  ]
    .filter(Boolean)
    .join(" · ");
}

/** One fixed row: Antigravity has no token, cost or quota, only how many conversations it has had. */
export function AntigravityRow({ activity }: { activity: AntigravityActivity }) {
  const { t, f } = useI18n();
  return (
    <section data-testid="antigravity-row" className="min-w-0 px-5 py-[13px]">
      <div className={HEAD}>
        <ServiceTile service="antigravity" size={26} />
        <span className="truncate text-[14.5px] font-semibold tracking-[-0.01em]">Antigravity</span>
        <Badge variant="secondary">{t("activity")}</Badge>
      </div>
      <p className="mt-[7px] break-words pl-[35px] text-[12.5px] tabular-nums text-muted-foreground">{antigravityText(activity, t, f)}</p>
    </section>
  );
}
