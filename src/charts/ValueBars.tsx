import type { PlanValue } from "@/lib/api";
import { useI18n, type Fmt, type T } from "@/lib/i18n";

const addDays = (iso: string, n: number): string => {
  const [y, m, d] = iso.split("-").map(Number);
  const t = new Date(y, m - 1, d + n);
  return `${t.getFullYear()}-${String(t.getMonth() + 1).padStart(2, "0")}-${String(t.getDate()).padStart(2, "0")}`;
};

/** "Siklus 1–30 Nov · Hari ke-12" ("Cycle Nov 1–30 · Day 12"); spans two months as "15 Okt–14 Nov" ("Oct 15–Nov 14"). */
export function cycleLabel(c: PlanValue["cycle"], t: T, f: Fmt): string {
  const { day: sd, month: sm } = f.dayMonth(c.start);
  const { day: ed, month: em } = f.dayMonth(addDays(c.endExclusive, -1));
  const range = sm === em ? t("rangeOneMonth", { sd, ed, m: em }) : t("rangeTwoMonths", { sd, sm, ed, em });
  return t("cycleLabel", { range, day: c.dayOfCycle });
}

/** Per plan: API-equivalent value as a bar on a muted track, a 2px tick at the price paid. */
export function ValueBars({ plans }: { plans: PlanValue[] }) {
  const { t, f } = useI18n();
  const max = Math.max(1e-9, ...plans.flatMap((p) => [p.apiEquivalentUsd, p.priceUsd])) * 1.05;
  return (
    <ul className="grid grid-cols-1 gap-5">
      {plans.map((p) => {
        const mult = f.multiple(p.multiple);
        return (
          <li key={p.planId}>
            <div className="flex items-baseline justify-between gap-3">
              <span className="truncate text-[13.5px] font-semibold">{p.name}</span>
              <span className="shrink-0 text-[13.5px] font-semibold tabular-nums">{mult}</span>
            </div>
            <p className="mb-2 text-xs tabular-nums text-muted-foreground">{cycleLabel(p.cycle, t, f)}</p>
            <div
              role="img"
              aria-label={t("planBarAria", { name: p.name, paid: f.usd0(p.priceUsd), worth: f.usd0(p.apiEquivalentUsd), x: f.multiple(p.multiple).replace("×", "") })}
              className="relative h-2.5 rounded-full bg-muted"
            >
              <div
                data-testid="value-fill"
                className="h-full rounded-full"
                style={{ width: `${Math.min(100, (p.apiEquivalentUsd / max) * 100)}%`, background: `var(--a-${p.agents[0] ?? "cc"})` }}
              />
              <i
                data-testid="price-tick"
                aria-hidden
                className="absolute -top-[3px] h-4 w-0.5 -translate-x-1/2 rounded-[1px] bg-foreground outline-[1.5px] outline-background"
                style={{ left: `${(p.priceUsd / max) * 100}%` }}
              />
            </div>
            <p className="mt-1.5 text-xs tabular-nums text-muted-foreground">{t("planPaidArrow", { paid: f.usd0(p.priceUsd), worth: f.usd0(p.apiEquivalentUsd) })}</p>
            <p className="text-xs tabular-nums text-muted-foreground">{t("planProjection", { a: f.usd0(p.projectedUsd), x: f.multiple(p.projectedMultiple) })}</p>
          </li>
        );
      })}
    </ul>
  );
}
