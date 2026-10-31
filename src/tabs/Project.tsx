import { useState } from "react";
import { RankedBar } from "@/charts/RankedBar";
import { Callout } from "@/components/Callout";
import { DataView } from "@/components/DataView";
import { PeriodSelect } from "@/components/PeriodSelect";
import { Unpriced } from "@/components/Unpriced";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useTabData } from "@/hooks/useTabData";
import { useUsdToIdr } from "@/hooks/useUsdToIdr";
import { AGENT_NAME, STACK_ORDER } from "@/lib/agents";
import { getProjects, type Period, type ProjectRow, type ProjectsView } from "@/lib/api";
import { isUnpricedOnly } from "@/lib/format";
import { useI18n, type Fmt, type Key, type T } from "@/lib/i18n";

export function Project() {
  const [period, setPeriod] = useState<Period>("7d");
  const state = useTabData(() => getProjects(period), [period]);
  const rate = useUsdToIdr();
  return (
    <TooltipProvider>
      <DataView state={state}>{(v) => <Body v={v} rate={rate} period={period} onPeriod={setPeriod} />}</DataView>
    </TooltipProvider>
  );
}

const rowKey = (r: ProjectRow) => `${r.folded}:${r.project.path}:${r.project.name}`;
const projectCount = (rows: ProjectRow[]) => rows.reduce((n, r) => n + (r.folded > 0 ? r.folded : 1), 0);
/** English needs "1 project" and "2 projects"; Indonesian words both the same. */
const plural = (t: T, one: Key, many: Key, n: number): string => t(n === 1 ? one : many, { n });

function trend(share: number, prev: number, t: T, f: Fmt): string {
  const [a, b] = [Math.round(share), Math.round(prev)];
  return a === b ? t("trendSame") : t(a > b ? "trendUp" : "trendDown", { pct: f.pct(prev) });
}

function Body({ v, rate, period, onPeriod }: { v: ProjectsView; rate: number; period: Period; onPeriod: (p: Period) => void }) {
  const { t, f } = useI18n();
  const max = Math.max(0, ...v.rows.map((r) => r.costUsd));
  const used = STACK_ORDER.filter((_, i) => v.rows.some((r) => r.byAgent[i] > 0));
  const ins = v.insight;
  return (
    <div className="px-5 pb-5 pt-[18px]">
      <header className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-[15px] font-semibold">{t("costByProject")}</h2>
          <p className="mt-0.5 text-[13px] tabular-nums text-muted-foreground">
            {isUnpricedOnly(v.totalCostUsd, v.rows.reduce((a, r) => a + r.unpricedTokens, 0)) ? (
              <>
                <Unpriced />
                {` · ${plural(t, "projectCountOne", "projectCountMany", projectCount(v.rows))}`}
              </>
            ) : (
              <span>{`${f.usd(v.totalCostUsd)} · ${f.idr(v.totalCostUsd, rate)} · ${plural(t, "projectCountOne", "projectCountMany", projectCount(v.rows))}`}</span>
            )}
          </p>
        </div>
        <PeriodSelect value={period} onChange={onPeriod} />
      </header>

      {v.rows.length === 0 ? (
        <p className="py-8 text-center text-[13px] text-muted-foreground">{t("noProjectUsage")}</p>
      ) : (
        <ul className="mt-3 grid grid-cols-1 gap-3.5">
          {v.rows.map((r) => {
            const name = r.folded > 0 ? plural(t, "otherProjectsOne", "otherProjectsMany", r.folded) : r.project.name;
            return (
              <li key={rowKey(r)}>
                <div className="flex items-center justify-between gap-3">
                  <span className="truncate text-[13.5px] font-medium">{name}</span>
                  <span className="shrink-0 text-[13.5px] font-semibold tabular-nums">{isUnpricedOnly(r.costUsd, r.unpricedTokens) ? <Unpriced /> : f.usd(r.costUsd)}</span>
                </div>
                <div className="my-1.5">
                  <RankedBar title={name} parts={STACK_ORDER.map((agent, i) => ({ agent, value: r.byAgent[i] }))} max={max} />
                </div>
                <div className="flex items-center justify-between gap-3 text-xs tabular-nums text-muted-foreground">
                  <span className="truncate">{r.folded > 0 ? "" : r.project.displayPath}</span>
                  <span className="shrink-0">{`${plural(t, "sessionCountOne", "sessionCountMany", r.sessions)} · ${f.pct(r.share)}`}</span>
                </div>
              </li>
            );
          })}
        </ul>
      )}

      {used.length > 0 && (
        <ul data-testid="project-legend" className="mt-4 flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
          {used.map((a) => (
            <li key={a} className="flex items-center gap-1.5">
              <i aria-hidden className="size-2 rounded-[2px]" style={{ background: `var(--a-${a})` }} />
              {AGENT_NAME[a]}
            </li>
          ))}
        </ul>
      )}

      {ins && (
        <div className="mt-4">
          <Callout>
            {t.rich("projectInsight", { name: ins.project.name, share: f.pct(ins.share), trend: trend(ins.share, ins.prevShare, t, f) })}
          </Callout>
        </div>
      )}
    </div>
  );
}
