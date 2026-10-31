import { Cell, Pie, PieChart } from "recharts";
import { ChartContainer, ChartTooltip, type ChartConfig } from "@/components/ui/chart";
import { useReducedMotion } from "@/hooks/useReducedMotion";
import { AGENT_NAME, STACK_ORDER } from "@/lib/agents";
import type { AgentKey } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { ChartTip } from "./ChartTip";

export interface AgentDonutProps {
  /** Cost per agent following STACK_ORDER. Agents without cost get no segment. */
  byAgent: readonly number[];
  /** Text in the hole, e.g. "$18,40", or a dash when nothing is priced. */
  centre: string;
  "aria-label": string;
}

const SIZE = 124;
/** Ring edges in px, scaled from the 72-unit mockup (r 29, stroke 9). */
const INNER = 42;
const OUTER = 58;
const GAP_DEG = 2;

const config = Object.fromEntries(STACK_ORDER.map((k) => [k, { label: AGENT_NAME[k], color: `var(--a-${k})` }])) as ChartConfig;

/** Largest figure size that still fits the ~84px hole: 17px up to 6 characters, then shrinking. */
export const centreFontSize = (text: string) => Math.min(17, Math.floor(72 / (Math.max(text.length, 1) * 0.58)));

/** Cost share per agent, largest first, as a ring with a gap between segments; the total sits in the hole. */
export function AgentDonut({ byAgent, centre, "aria-label": ariaLabel }: AgentDonutProps) {
  const reduced = useReducedMotion();
  const { t, f } = useI18n();
  // `sort` is stable: equal costs keep STACK_ORDER.
  const segments = STACK_ORDER.flatMap((agent, i) => ((byAgent[i] ?? 0) > 0 ? [{ agent, value: byAgent[i] }] : [])).sort((a, b) => b.value - a.value);
  const sum = segments.reduce((a, s) => a + s.value, 0);
  const empty = segments.length === 0;
  const data: { agent: AgentKey | null; value: number }[] = empty ? [{ agent: null, value: 1 }] : segments;

  return (
    <div role="img" aria-label={ariaLabel} className="relative shrink-0" style={{ width: SIZE, height: SIZE }}>
      {/* The hole's text comes first and the chart stacks above it, so the tooltip always covers it. */}
      <div className="absolute inset-0 grid place-content-center text-center">
        <b className="font-semibold leading-none tracking-[-0.02em] tabular-nums" style={{ fontSize: centreFontSize(centre) }}>
          {centre}
        </b>
        <span className="mt-1 text-[10.5px] leading-none text-muted-foreground">{t("today")}</span>
      </div>
      <ChartContainer config={config} className="relative aspect-auto size-full">
        <PieChart accessibilityLayer={false} margin={{ top: 0, right: 0, bottom: 0, left: 0 }}>
          <ChartTooltip
            isAnimationActive={false}
            allowEscapeViewBox={{ x: true, y: true }}
            wrapperStyle={{ zIndex: 20 }}
            content={({ active, payload }) => {
              const seg = payload?.[0]?.payload as { agent: AgentKey | null; value: number } | undefined;
              if (!active || !seg?.agent) return null;
              return (
                <ChartTip
                  title={AGENT_NAME[seg.agent]}
                  rows={[
                    { color: `var(--a-${seg.agent})`, label: t("chartCost"), value: f.usd(seg.value) },
                    { label: t("chartShare"), value: f.pct((seg.value / sum) * 100) },
                  ]}
                />
              );
            }}
          />
          <Pie
            data={data}
            dataKey="value"
            nameKey="agent"
            cx="50%"
            cy="50%"
            innerRadius={INNER}
            outerRadius={OUTER}
            startAngle={90}
            endAngle={-270}
            paddingAngle={empty || segments.length === 1 ? 0 : GAP_DEG}
            stroke="none"
            isAnimationActive={!reduced}
          >
            {data.map((d) => (
              <Cell key={d.agent ?? "none"} fill={d.agent ? `var(--a-${d.agent})` : "var(--muted)"} />
            ))}
          </Pie>
        </PieChart>
      </ChartContainer>
    </div>
  );
}
