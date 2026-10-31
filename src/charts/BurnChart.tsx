import { useId } from "react";
import { Area, AreaChart, CartesianGrid, ReferenceDot, XAxis, YAxis } from "recharts";
import { ChartContainer, ChartTooltip, type ChartConfig } from "@/components/ui/chart";
import { useReducedMotion } from "@/hooks/useReducedMotion";
import type { Burn } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { ChartTip } from "./ChartTip";

const MIN = 60_000;
interface Row {
  x: number;
  actual?: number;
  proj?: number;
}

/** Usage so far (area + 2px line) and a dotted projection to the reset, red when the quota runs out first. */
export function BurnChart({ burn, color = "var(--a-cc)", height = 134 }: { burn: Burn; color?: string; height?: number }) {
  const reduced = useReducedMotion();
  const { t, f } = useI18n();
  const config = {
    actual: { label: t("seriesUsed"), color: "var(--a-cc)" },
    proj: { label: t("seriesProjection"), color: "var(--a-cc)" },
  } satisfies ChartConfig;
  const gradId = `burn-${useId().replace(/:/g, "")}`;
  const start = new Date(burn.windowStart).getTime();
  const total = Math.max(1, (new Date(burn.resetsAt).getTime() - start) / MIN);
  const exhaustX = burn.exhaustsAt ? Math.min(total, Math.max(0, (new Date(burn.exhaustsAt).getTime() - start) / MIN)) : null;
  const warn = burn.exhaustsAt !== null;
  const projColor = warn ? "var(--destructive)" : color;

  const last = burn.points[burn.points.length - 1];
  const nowX = last?.[0] ?? 0;
  const end: [number, number] = exhaustX !== null ? [exhaustX, 100] : [total, Math.min(100, burn.projectedAtReset)];

  const rows: Row[] = burn.points.map(([x, v]) => ({ x, actual: v }));
  if (last) {
    rows[rows.length - 1] = { ...rows[rows.length - 1], proj: last[1] };
    if (end[0] > nowX) rows.push({ x: end[0], proj: end[1] });
  }

  const at = (x: number) => f.clock(new Date(start + x * MIN).toISOString());
  const endLabel = warn ? t("burnExhausted", { time: f.clock(burn.exhaustsAt!) }) : t("burnAtReset", { n: Math.round(end[1]) });
  // "Sekarang" ("Now") is dropped when it sits too close to either end label (~20% of the plot) to avoid overlap.
  const showNow = !!last && nowX / total >= 0.2 && (total - nowX) / total >= 0.2;
  const xTicks = showNow ? [0, nowX, total] : [0, total];
  const xLabel = (v: number) => (v === 0 ? at(0) : v === total ? at(total) : t("burnNow"));
  // First label starts at the plot edge and the last ends at it, so neither is clipped.
  const xTick = ({ x, y, payload }: { x: number; y: number; payload: { value: number } }) => (
    <text
      x={x}
      y={y}
      dy="0.71em"
      fontSize={12}
      textAnchor={payload.value === 0 ? "start" : payload.value === total ? "end" : "middle"}
      style={{ fill: "var(--muted-foreground)" }}
    >
      {xLabel(payload.value)}
    </text>
  );

  return (
    <ChartContainer
      config={config}
      className="aspect-auto w-full"
      style={{ height }}
      role="group"
      aria-label={t("burnAria", { end: endLabel })}
    >
      <AreaChart accessibilityLayer data={rows} margin={{ top: 24, right: 0, bottom: 0, left: 0 }}>
        <defs>
          <linearGradient id={gradId} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={color} stopOpacity={0.28} />
            <stop offset="100%" stopColor={color} stopOpacity={0.02} />
          </linearGradient>
        </defs>
        <CartesianGrid vertical={false} stroke="var(--border)" />
        <XAxis
          type="number"
          dataKey="x"
          domain={[0, total]}
          ticks={xTicks}
          tick={xTick as never}
          tickLine={false}
          axisLine={false}
          tickMargin={6}
          interval={0}
          fontSize={12}
        />
        <YAxis
          orientation="right"
          domain={[0, 100]}
          ticks={[0, 50, 100]}
          tickFormatter={(v: number) => (v === 100 ? t("burnLimit") : `${v}%`)}
          tickLine={false}
          axisLine={false}
          width={46}
          tickMargin={8}
          fontSize={12}
        />
        <ChartTooltip
          isAnimationActive={false}
          cursor={{ stroke: "var(--border)" }}
          content={({ active, label }) => {
            if (!active || typeof label !== "number") return null;
            const future = label > nowX;
            const row = rows.find((r) => r.x === label);
            const v = Math.round(future ? (row?.proj ?? 0) : (row?.actual ?? row?.proj ?? 0));
            return (
              <ChartTip
                title={future ? t("burnTipProjection", { time: at(label) }) : at(label)}
                rows={[
                  { color: projColor, label: future ? t("burnProjectedUsed") : t("seriesUsed"), value: `${v}%` },
                  { label: t("left"), value: `${Math.max(0, 100 - v)}%` },
                ]}
              />
            );
          }}
        />
        <Area
          dataKey="actual"
          type="monotone"
          stroke={color}
          strokeWidth={2}
          strokeLinecap="round"
          fill={`url(#${gradId})`}
          dot={false}
          activeDot={{ r: 4, stroke: "var(--background)", strokeWidth: 2 }}
          isAnimationActive={!reduced}
          connectNulls
        />
        <Area
          dataKey="proj"
          type="linear"
          stroke={projColor}
          strokeWidth={2}
          strokeDasharray="2 5"
          strokeLinecap="round"
          fill="none"
          dot={false}
          activeDot={false}
          isAnimationActive={false}
          connectNulls
        />
        {last && <ReferenceDot x={nowX} y={last[1]} r={4.5} fill={color} stroke="var(--background)" strokeWidth={2} ifOverflow="visible" />}
        {last && end[0] > nowX && (
          <ReferenceDot
            x={end[0]}
            y={end[1]}
            r={0}
            ifOverflow="visible"
            label={(p: { viewBox?: { x?: number; y?: number } }) => (
              <text
                x={(p.viewBox?.x ?? 0) - 4}
                y={(p.viewBox?.y ?? 0) - 10}
                textAnchor="end"
                fontSize={12}
                fontWeight={600}
                style={{ fill: warn ? "var(--destructive)" : "var(--foreground)" }}
              >
                {endLabel}
              </text>
            )}
          />
        )}
      </AreaChart>
    </ChartContainer>
  );
}
