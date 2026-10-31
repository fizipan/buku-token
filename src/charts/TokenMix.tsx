import { ChartTip } from "@/charts/ChartTip";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { Mix } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

/** 14px stacked bar: cache read in foreground, the rest in a muted tone. Needs a TooltipProvider above. */
export function TokenMix({ label, mix }: { label: string; mix: Mix }) {
  const { t, f } = useI18n();
  const rest = mix.input + mix.output + mix.cacheWrite;
  const summary = t("mixSummary", { label, cacheRead: f.pct(mix.cacheRead), input: f.pct(mix.input), output: f.pct(mix.output), cacheWrite: f.pct(mix.cacheWrite) });
  return (
    <div className="grid grid-cols-[48px_1fr] items-center gap-2.5 text-[13px]">
      <span className="text-muted-foreground">{label}</span>
      <Tooltip>
        <TooltipTrigger asChild>
          <div
            role="img"
            tabIndex={0}
            aria-label={summary}
            className="flex h-3.5 gap-0.5 overflow-hidden rounded-[4px] outline-none focus-visible:ring-[3px] focus-visible:ring-ring"
          >
            <span className="h-full bg-foreground" style={{ flex: `${mix.cacheRead} 1 0` }} />
            <span className="h-full bg-muted-foreground/30" style={{ flex: `${rest} 1 0` }} />
          </div>
        </TooltipTrigger>
        <TooltipContent side="top" className="bg-transparent p-0 text-foreground">
          <ChartTip
            title={label}
            rows={[
              { label: t("cacheRead"), value: f.pct(mix.cacheRead) },
              { label: t("input"), value: f.pct(mix.input) },
              { label: t("output"), value: f.pct(mix.output) },
              { label: t("cacheWrite"), value: f.pct(mix.cacheWrite) },
            ]}
          />
        </TooltipContent>
      </Tooltip>
    </div>
  );
}
