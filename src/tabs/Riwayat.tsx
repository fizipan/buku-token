import { useState } from "react";
import { StackedColumns } from "@/charts/StackedColumns";
import { ValueBars } from "@/charts/ValueBars";
import { DataView } from "@/components/DataView";
import { PeriodSelect } from "@/components/PeriodSelect";
import { Unpriced } from "@/components/Unpriced";
import { Badge } from "@/components/ui/badge";
import { useTabData } from "@/hooks/useTabData";
import { useUsdToIdr } from "@/hooks/useUsdToIdr";
import { getHistory, type HistoryView, type Period } from "@/lib/api";
import { isUnpricedOnly } from "@/lib/format";
import { useI18n } from "@/lib/i18n";

const OPTIONS = ["7d", "14d", "30d"] as const satisfies readonly Period[];
const DAYS: Record<(typeof OPTIONS)[number], number> = { "7d": 7, "14d": 14, "30d": 30 };

/** Column indices to label: every `every`-th from the first, plus the last; a regular label closer than half an interval to the last is dropped. */
export function axisLabelIndices(n: number, every: number): number[] {
  if (n <= 0) return [];
  const last = n - 1;
  const regular = Array.from({ length: Math.ceil(n / every) }, (_, i) => i * every).filter((i) => i === last || last - i >= every / 2);
  return regular.includes(last) ? regular : [...regular, last];
}

export function Riwayat() {
  const [period, setPeriod] = useState<(typeof OPTIONS)[number]>("30d");
  const state = useTabData(() => getHistory(DAYS[period]), [period]);
  return <DataView state={state}>{(v) => <Body v={v} period={period} onPeriod={setPeriod} />}</DataView>;
}

function Body({ v, period, onPeriod }: { v: HistoryView; period: (typeof OPTIONS)[number]; onPeriod: (p: (typeof OPTIONS)[number]) => void }) {
  const rate = useUsdToIdr();
  const { t, f } = useI18n();
  const dates = v.days.map((d) => d.date);
  const paid = v.plans.reduce((a, p) => a + p.priceUsd, 0);
  const worth = v.plans.reduce((a, p) => a + p.apiEquivalentUsd, 0);
  const noPrice = isUnpricedOnly(v.totalUsd, v.unpricedTokens);
  return (
    <div className="grid grid-cols-1 gap-6 px-5 pb-5 pt-[18px]">
      <section>
        <header className="flex items-start justify-between gap-3">
          <h2 className="text-[15px] font-semibold">{t("dailyCost")}</h2>
          <PeriodSelect value={period} onChange={(p) => onPeriod(p as (typeof OPTIONS)[number])} options={OPTIONS} />
        </header>
        <div className="mt-1 text-[26px] font-semibold leading-tight tracking-[-0.03em] [font-variant-numeric:proportional-nums]">{noPrice ? <Unpriced /> : f.usd(v.totalUsd)}</div>
        {!noPrice && <p className="mt-1 text-[13px] tabular-nums text-muted-foreground">{t("avgPerDay", { idr: f.idr(v.totalUsd, rate), usd: f.usd(v.avgPerDay) })}</p>}
        <div className="mt-4">
          <StackedColumns
            aria-label={t("dailyCostAria")}
            data={v.days.map((d) => ({ label: String(Number(d.date.slice(8, 10))), byAgent: d.byAgent, unpriced: d.unpricedTokens > 0 }))}
            highlightIndex={v.days.length - 1}
            labelIndices={axisLabelIndices(v.days.length, period === "7d" ? 1 : 7)}
            valueFormat={f.usd0}
            tooltipFormat={f.usd}
            tooltipTitle={(_, i) => (dates[i] ? f.dayLabel(dates[i]) : "")}
            height={150}
          />
        </div>
      </section>

      {v.plans.length > 0 && (
        <section>
          <header className="mb-3 flex items-start justify-between gap-3">
            <div>
              <h2 className="text-[15px] font-semibold">{t("planValue")}</h2>
              <p className="mt-0.5 text-[13px] tabular-nums text-muted-foreground">{t("planPaid", { paid: f.usd0(paid), worth: f.usd0(worth) })}</p>
            </div>
            {paid > 0 && <Badge variant="secondary" className="h-6 text-[13px] font-semibold tabular-nums">{t("payback", { x: f.multiple(worth / paid) })}</Badge>}
          </header>
          <ValueBars plans={v.plans} />
        </section>
      )}
    </div>
  );
}
