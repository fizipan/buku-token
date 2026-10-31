import { Bar, BarChart, CartesianGrid, Rectangle, XAxis, YAxis, type BarShapeProps, type XAxisTickContentProps } from "recharts";
import { ChartContainer, ChartTooltip, type ChartConfig } from "@/components/ui/chart";
import { useReducedMotion } from "@/hooks/useReducedMotion";
import { AGENT_NAME, STACK_ORDER } from "@/lib/agents";
import { useT } from "@/lib/i18n";
import { ChartTip } from "./ChartTip";

export interface StackedColumnsProps {
  /** One column each; `byAgent` follows STACK_ORDER [cc, cx, oc, hm, cr]. `unpriced`: it also has tokens without a price. */
  data: { label: string; byAgent: number[]; unpriced?: boolean }[];
  /** Column whose x label is drawn bold (e.g. the current hour). */
  highlightIndex?: number;
  /** Axis ticks and the default tooltip value, e.g. usd0 -> "$2". */
  valueFormat: (n: number) => string;
  /** Distance between y ticks in value units. Default: a 1/2/5 step giving about 3 intervals. */
  tickStep?: number;
  /** Tooltip value format (default: valueFormat), e.g. usd -> "$3,50". */
  tooltipFormat?: (n: number) => string;
  /** Tooltip heading for a column (default: its label). */
  tooltipTitle?: (label: string, index: number) => string;
  /** Show every n-th x label (default 3). */
  labelEvery?: number;
  /** Exact column indices whose x label is drawn; overrides labelEvery. */
  labelIndices?: readonly number[];
  /** Chart height in px (default 132). */
  height?: number;
  /** Agent legend below the chart (default true). */
  showLegend?: boolean;
  "aria-label"?: string;
}

const config = Object.fromEntries(STACK_ORDER.map((k) => [k, { label: AGENT_NAME[k], color: `var(--a-${k})` }])) as ChartConfig;

function niceStep(rough: number): number {
  const mag = 10 ** Math.floor(Math.log10(rough));
  return ([1, 2, 5, 10].find((m) => m * mag >= rough) ?? 10) * mag;
}

const AXIS_MARGIN = 6;

/** Y axis width for its longest tick label: margin + 7.5px per 12px Geist character + 2px breathing room. */
export function yAxisWidth(labels: string[]): number {
  return AXIS_MARGIN + Math.ceil(Math.max(0, ...labels.map((l) => l.length)) * 7.5) + 2;
}

type Row = Record<string, number | string>;

/** Vertical stacked columns by agent: 24px max, 4px top radius, 2px surface gap, agent legend below. */
export function StackedColumns({
  data,
  highlightIndex,
  valueFormat,
  tickStep,
  tooltipFormat = valueFormat,
  tooltipTitle = (l) => l,
  labelEvery = 3,
  labelIndices,
  height = 132,
  showLegend = true,
  "aria-label": ariaLabel,
}: StackedColumnsProps) {
  const reduced = useReducedMotion();
  const t = useT();
  const rows: Row[] = data.map((d) => ({ label: d.label, ...Object.fromEntries(STACK_ORDER.map((k, i) => [k, d.byAgent[i] ?? 0])) }));
  const totals = rows.map((r) => STACK_ORDER.reduce((a, k) => a + (r[k] as number), 0));
  const max = Math.max(0, ...totals);
  const step = tickStep ?? niceStep(Math.max(max, 1e-9) / 3);
  const top = Math.max(step, Math.ceil(max / step - 1e-9) * step);
  const ticks = Array.from({ length: Math.round(top / step) + 1 }, (_, i) => i * step);
  const labelled = labelIndices ? new Set(labelIndices) : null;
  const present = STACK_ORDER.filter((k) => rows.some((r) => (r[k] as number) > 0));

  return (
    <div>
      <ChartContainer config={config} className="aspect-auto w-full" style={{ height }} role="group" aria-label={ariaLabel}>
        <BarChart accessibilityLayer data={rows} margin={{ top: 10, right: 0, bottom: 0, left: 0 }} barCategoryGap={2}>
          <CartesianGrid vertical={false} stroke="var(--border)" />
          <XAxis
            dataKey="label"
            interval={0}
            tickLine={false}
            axisLine={{ stroke: "var(--border)" }}
            tickMargin={6}
            tick={(p: XAxisTickContentProps) =>
              (labelled ? labelled.has(p.index ?? -1) : (p.index ?? 0) % labelEvery === 0) ? (
                <text
                  x={Number(p.x)}
                  y={Number(p.y) + 10}
                  textAnchor="middle"
                  fontSize={12}
                  fontWeight={p.index === highlightIndex ? 600 : 400}
                  style={{ fill: p.index === highlightIndex ? "var(--foreground)" : "var(--muted-foreground)" }}
                >
                  {String(p.payload.value)}
                </text>
              ) : (
                <g />
              )
            }
          />
          <YAxis
            orientation="right"
            domain={[0, top]}
            ticks={ticks}
            tickFormatter={valueFormat}
            tickLine={false}
            tickSize={0}
            axisLine={false}
            width={yAxisWidth(ticks.map(valueFormat))}
            tickMargin={AXIS_MARGIN}
            fontSize={12}
          />
          <ChartTooltip
            isAnimationActive={false}
            content={({ active, label, payload }) => {
              if (!active || !payload?.length) return null;
              const row = payload[0].payload as Row;
              const index = rows.indexOf(row);
              const parts = STACK_ORDER.filter((k) => (row[k] as number) > 0);
              return (
                <ChartTip
                  title={tooltipTitle(String(label), index)}
                  rows={
                    parts.length
                      ? [
                          ...parts.map((k) => ({ color: `var(--a-${k})`, label: AGENT_NAME[k], value: tooltipFormat(row[k] as number) })),
                          { label: t("chartSum"), value: tooltipFormat(totals[index] ?? 0), strong: true },
                        ]
                      : [{ label: data[index]?.unpriced ? t("noPrice") : t("chartNoSessions"), value: "–" }]
                  }
                />
              );
            }}
          />
          {STACK_ORDER.map((k, j) => (
            <Bar
              key={k}
              dataKey={k}
              stackId="a"
              fill={`var(--a-${k})`}
              maxBarSize={24}
              isAnimationActive={!reduced}
              shape={(p: BarShapeProps) => {
                const v = p.payload as Record<string, number>;
                if (!(v[k] > 0) || !(p.height > 0)) return <g />;
                const isTop = STACK_ORDER.slice(j + 1).every((n) => !(v[n] > 0));
                return (
                  <Rectangle
                    x={p.x}
                    y={p.y}
                    width={p.width}
                    height={p.height}
                    fill={`var(--a-${k})`}
                    stroke="var(--background)"
                    strokeWidth={2}
                    radius={isTop ? [4, 4, 0, 0] : 0}
                  />
                );
              }}
            />
          ))}
        </BarChart>
      </ChartContainer>
      {showLegend && (
        <ul data-testid="stacked-legend" className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
          {present.map((k) => (
            <li key={k} className="flex items-center gap-1.5">
              <i aria-hidden className="inline-block size-[9px] rounded-[2px]" style={{ background: `var(--a-${k})` }} />
              {AGENT_NAME[k]}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
