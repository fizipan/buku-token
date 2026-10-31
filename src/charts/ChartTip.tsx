import { cn } from "cn";

export interface TipRow {
  /** CSS color of the key square; omitted for rows without a series (totals, remaining). */
  color?: string;
  label: string;
  value: string;
  strong?: boolean;
}

/** Tooltip body in the shadcn chart tooltip style, shared by the Ringkasan charts. */
export function ChartTip({ title, rows, className }: { title: string; rows: TipRow[]; className?: string }) {
  return (
    <div
      className={cn(
        "grid min-w-32 gap-1.5 rounded-lg border border-border/50 bg-background px-2.5 py-1.5 text-xs shadow-xl",
        className,
      )}
    >
      <div className="font-medium">{title}</div>
      {rows.map((r) => (
        <div key={r.label} className="flex items-center justify-between gap-4">
          <span className="flex items-center gap-1.5 text-muted-foreground">
            {r.color && <i aria-hidden className="inline-block size-2 shrink-0 rounded-[2px]" style={{ background: r.color }} />}
            {r.label}
          </span>
          <span className={cn("font-mono tabular-nums text-foreground", r.strong ? "font-semibold" : "font-medium")}>{r.value}</span>
        </div>
      ))}
    </div>
  );
}
