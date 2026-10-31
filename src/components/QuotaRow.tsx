import type { AgentKey, QuotaDisplay, QuotaWindow, ResetDisplay } from "@/lib/api";
import { useI18n, type T } from "@/lib/i18n";
import { isExpiredWindow, isStaleWindow, remainingPct } from "@/lib/quota";
import { Progress } from "@/components/ui/progress";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

const DAY_MS = 86_400_000;
/** Remaining share at or below which "Sisa N%" ("Left N%") turns red. */
const LOW_LEFT = 15;

export interface QuotaRowProps {
  agent: AgentKey;
  window: QuotaWindow;
  /** Share of the week already elapsed (0..100); draws the pace tick. Weekly windows only. */
  pace?: number | null;
  now: Date;
  /** The section header already says when every window resets: drop the last column. */
  hideReset?: boolean;
  /** Sisa (default) or Terpakai. */
  quotaDisplay?: QuotaDisplay;
  /** Jam (default) or Hitung Mundur for the reset column. */
  resetDisplay?: ResetDisplay;
}

/** "2j 15m", or "3 hari" from a day out. */
export function countdown(resetsAt: string, now: Date, t: T, duration: (min: number) => string): string {
  const min = Math.max(0, (new Date(resetsAt).getTime() - now.getTime()) / 60_000);
  return min >= 1440 ? t("daysCount", { n: Math.floor(min / 1440) }) : duration(min);
}

/**
 * The value column fits "Sisa 62%" (58px) or "Terpakai 100%" (88px); the bar gives way, not the text.
 * English is shorter: "Left 100%" fits 58px and "Used 100%" needs only 64px, so the bar gets 24px back.
 */
const GRID = {
  id: {
    left: { three: "grid-cols-[92px_1fr_58px]", four: "grid-cols-[92px_1fr_58px_62px]" },
    used: { three: "grid-cols-[92px_1fr_88px]", four: "grid-cols-[92px_1fr_88px_62px]" },
  },
  en: {
    left: { three: "grid-cols-[92px_1fr_58px]", four: "grid-cols-[92px_1fr_58px_62px]" },
    used: { three: "grid-cols-[92px_1fr_64px]", four: "grid-cols-[92px_1fr_64px_62px]" },
  },
} as const;

/**
 * One quota line: label, agent-colored bar (optional pace tick), "Sisa N%", reset time.
 * A stale or expired window shows the date its data is from, in red, instead of the reset time.
 * Past its reset the old percentage means nothing: an empty bar and a sentence instead.
 */
export function QuotaRow({ agent, window: w, pace = null, now, hideReset = false, quotaDisplay = "left", resetDisplay = "clock" }: QuotaRowProps) {
  const { lang, t, f } = useI18n();
  const expired = isExpiredWindow(w, now);
  const stale = isStaleWindow(w, now);
  const used = Math.min(100, Math.max(0, w.usedPct));
  const left = remainingPct(w.usedPct);
  const usedRounded = Math.round(used);
  const diff = pace === null ? 0 : Math.round(used - pace);
  const color = `var(--a-${agent})`;
  const track = { "--bar": color, background: `color-mix(in oklab, ${color} 16%, var(--background))` } as React.CSSProperties;
  const dataDate = `${t("lastData")} ${f.shortDate(w.observedAt)}`;
  const farAway = new Date(w.resetsAt).getTime() - now.getTime() > DAY_MS;
  return (
    <div data-testid="quota-row" className={`grid min-w-0 items-center gap-x-2.5 text-[12.5px] ${GRID[lang][quotaDisplay === "used" ? "used" : "left"][hideReset ? "three" : "four"]}`}>
      <span className="truncate capitalize text-muted-foreground" title={w.label}>
        {w.label}
      </span>
      {expired ? (
        <Progress value={0} aria-label={t("quotaResetAria", { label: w.label })} className="h-1.5 [&_[data-slot=progress-indicator]]:rounded-full" style={track} />
      ) : (
        <Tooltip>
          <TooltipTrigger asChild>
            <div
              tabIndex={0}
              role="progressbar"
              aria-label={w.label}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={usedRounded}
              aria-valuetext={t("quotaAria", { label: w.label, used: usedRounded, left })}
              className="relative rounded-full outline-none focus-visible:ring-[3px] focus-visible:ring-ring"
            >
              <Progress
                value={used}
                aria-hidden="true"
                className="h-1.5 [&_[data-slot=progress-indicator]]:rounded-full [&_[data-slot=progress-indicator]]:bg-(--bar)"
                style={track}
              />
              {pace !== null && (
                <span
                  data-testid="pace-tick"
                  aria-hidden
                  className="absolute -top-[3px] h-3 w-0.5 rounded-[1px] bg-foreground"
                  style={{ left: `calc(${pace}% - 1px)` }}
                />
              )}
            </div>
          </TooltipTrigger>
          <TooltipContent side="top" className="flex-col items-start gap-0.5 text-xs">
            <span>{t("quotaUsed", { n: usedRounded })}</span>
            {pace !== null && (
              <>
                <span>{t("quotaPace", { n: Math.round(pace) })}</span>
                <span>{t(diff > 0 ? "paceOver" : "paceUnder", { n: Math.abs(diff) })}</span>
              </>
            )}
          </TooltipContent>
        </Tooltip>
      )}
      <span className={`whitespace-nowrap text-right font-semibold ${!expired && left <= LOW_LEFT ? "text-destructive" : ""}`}>
        {expired ? <span className="font-normal text-muted-foreground">–</span> : quotaDisplay === "used" ? t("quotaUsed", { n: usedRounded }) : t("quotaLeft", { n: left })}
      </span>
      {hideReset ? null : stale ? (
        <span className="min-w-0 truncate text-right text-destructive" title={dataDate}>
          <span className="sr-only">{t("lastData")} </span>
          {f.shortDate(w.observedAt)}
        </span>
      ) : (
        <span className="min-w-0 truncate text-right text-muted-foreground">{resetDisplay === "countdown" ? countdown(w.resetsAt, now, t, f.duration) : farAway ? f.shortDate(w.resetsAt) : f.clock(w.resetsAt)}</span>
      )}
      {expired && <p className="col-span-full mt-1 text-xs text-muted-foreground">{t("resetNoData")}</p>}
    </div>
  );
}
