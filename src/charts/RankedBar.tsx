import { ChartTip, type TipRow } from "@/charts/ChartTip";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { AGENT_NAME } from "@/lib/agents";
import type { AgentKey } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

export interface RankedPart {
  agent: AgentKey;
  value: number;
}

interface RankedBarProps {
  /** Segments in stack order (cc, cx, oc, hm); zero values are skipped. */
  parts: RankedPart[];
  /** Value that fills the whole track. */
  max: number;
  /** Title in the tooltip and prefix of the aria-label. */
  title?: string;
  /** Defaults to the dollar format of the current language. */
  format?: (n: number) => string;
}

/** 8px stacked bar on a muted track: 2px gaps, ends clipped to a 4px radius. Needs a TooltipProvider above. */
export function RankedBar({ parts, max, title, format: formatProp }: RankedBarProps) {
  const { t, f } = useI18n();
  const format = formatProp ?? f.usd;
  const shown = parts.filter((p) => p.value > 0);
  const total = shown.reduce((a, p) => a + p.value, 0);
  const width = max > 0 ? Math.min(100, (total / max) * 100) : 0;
  const summary = shown.map((p) => `${AGENT_NAME[p.agent]} ${format(p.value)}`).join(", ") || t("chartNoCost");
  const rows: TipRow[] = shown.map((p) => ({ color: `var(--a-${p.agent})`, label: AGENT_NAME[p.agent], value: format(p.value) }));
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <div
          role="img"
          tabIndex={0}
          aria-label={title ? `${title}: ${summary}` : summary}
          className="h-2 overflow-hidden rounded-[4px] bg-muted outline-none focus-visible:ring-[3px] focus-visible:ring-ring"
        >
          <div className="flex h-full gap-0.5" style={{ width: `${width}%` }}>
            {shown.map((p) => (
              <span key={p.agent} className="h-full" style={{ flex: `${p.value} 1 0`, background: `var(--a-${p.agent})` }} />
            ))}
          </div>
        </div>
      </TooltipTrigger>
      {shown.length > 0 && (
        <TooltipContent side="top" className="bg-transparent p-0 text-foreground">
          <ChartTip title={title ?? t("chartCost")} rows={[...rows, ...(shown.length > 1 ? [{ label: t("chartTotal"), value: format(total), strong: true }] : [])]} />
        </TooltipContent>
      )}
    </Tooltip>
  );
}
