import { RankedBar } from "@/charts/RankedBar";
import { TokenMix } from "@/charts/TokenMix";
import { AgentTile } from "@/components/AgentTile";
import { Callout } from "@/components/Callout";
import { DataView } from "@/components/DataView";
import { PeriodSelect } from "@/components/PeriodSelect";
import { Unpriced } from "@/components/Unpriced";
import { Badge } from "@/components/ui/badge";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useTabData } from "@/hooks/useTabData";
import { useUsdToIdr } from "@/hooks/useUsdToIdr";
import { modelLabel } from "@/lib/agents";
import { getModels, type ModelRow, type ModelsView, type Period } from "@/lib/api";
import { isUnpricedOnly } from "@/lib/format";
import { useI18n } from "@/lib/i18n";
import { useState } from "react";

export function Model() {
  const [period, setPeriod] = useState<Period>("7d");
  const state = useTabData(() => getModels(period), [period]);
  const rate = useUsdToIdr();
  return (
    <TooltipProvider>
      <DataView state={state}>{(v) => <Body v={v} rate={rate} period={period} onPeriod={setPeriod} />}</DataView>
    </TooltipProvider>
  );
}

const byCost = (a: ModelRow, b: ModelRow) => (b.costUsd ?? -1) - (a.costUsd ?? -1);

function Body({ v, rate, period, onPeriod }: { v: ModelsView; rate: number; period: Period; onPeriod: (p: Period) => void }) {
  const { t, f } = useI18n();
  const rows = [...v.rows].sort(byCost);
  const max = Math.max(0, ...rows.map((r) => r.costUsd ?? 0));
  const ci = v.cacheInsight;
  return (
    <div className="px-5 pb-5 pt-[18px]">
      <header className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-[15px] font-semibold">{t("costByModel")}</h2>
          <p className="mt-0.5 text-[13px] tabular-nums text-muted-foreground">
            {isUnpricedOnly(v.totalCostUsd, v.unpricedTokens) ? <Unpriced /> : `${f.usd(v.totalCostUsd)} · ${f.idr(v.totalCostUsd, rate)}`}
          </p>
        </div>
        <PeriodSelect value={period} onChange={onPeriod} />
      </header>

      {rows.length === 0 ? (
        <p className="py-8 text-center text-[13px] text-muted-foreground">{t("noModelUsage")}</p>
      ) : (
        <ul className="mt-3 grid grid-cols-1 gap-3.5">
          {rows.map((r) => (
            <li key={`${r.agent}:${r.model}`} className="flex gap-3">
              <AgentTile agent={r.agent} model={r.model} size={24} />
              <div className="min-w-0 flex-1">
                <div className="flex items-center justify-between gap-3">
                  <span data-model className="truncate text-[13.5px] font-medium">
                    {modelLabel(r.model)}
                  </span>
                  <span className="flex shrink-0 items-center gap-1.5 tabular-nums">
                    {r.costUsd === null && <Badge variant="outline">{t("noPrice")}</Badge>}
                    <span className="text-[13.5px] font-semibold">{f.usd(r.costUsd)}</span>
                  </span>
                </div>
                <div className="my-1.5">
                  <RankedBar parts={[{ agent: r.agent, value: r.costUsd ?? 0 }]} max={max} />
                </div>
                <div className="flex items-center justify-between gap-3 text-xs tabular-nums text-muted-foreground">
                  <span className="truncate">{t("modelLine", { input: f.tokens(r.input), output: f.tokens(r.output), cache: f.tokens(r.cacheRead + r.cacheWrite) })}</span>
                  {r.share !== null && <span className="shrink-0">{f.pct(r.share)}</span>}
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}

      <section className="mt-6">
        <h2 className="mb-3 text-[15px] font-semibold">{t("whereTokensGo")}</h2>
        <div className="grid grid-cols-1 gap-2.5">
          <TokenMix label={t("mixTokens")} mix={v.tokenMix.tokens} />
          <TokenMix label={t("mixCost")} mix={v.tokenMix.cost} />
        </div>
        <div className="mt-2.5 flex gap-4 pl-[58px] text-xs text-muted-foreground">
          <span className="flex items-center gap-1.5">
            <i aria-hidden className="size-2 rounded-[2px] bg-foreground" />
            {t("cacheRead")}
          </span>
          <span className="flex items-center gap-1.5">
            <i aria-hidden className="size-2 rounded-[2px] bg-muted-foreground/30" />
            {t("mixRest")}
          </span>
        </div>
      </section>

      {ci && (
        <div className="mt-4">
          <Callout>
            {t.rich("cacheInsight", { tokenShare: f.pct(ci.tokenShare), costShare: f.pct(ci.costShare), without: f.usd(ci.costWithoutCache) })}
          </Callout>
        </div>
      )}
    </div>
  );
}
