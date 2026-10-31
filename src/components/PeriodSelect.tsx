import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { Period } from "@/lib/api";
import { useT, type Key } from "@/lib/i18n";

/** Dictionary key of each period's label. */
export const PERIOD_LABEL: Record<Period, Key> = {
  today: "today",
  "7d": "period7d",
  "14d": "period14d",
  "30d": "period30d",
};

interface PeriodSelectProps {
  value: Period;
  onChange: (p: Period) => void;
  options?: readonly Period[];
}

/** Period dropdown. Model/Project use today/7d/30d; Riwayat passes 7d/14d/30d. */
export function PeriodSelect({ value, onChange, options = ["today", "7d", "30d"] }: PeriodSelectProps) {
  const t = useT();
  return (
    <Select value={value} onValueChange={(v) => onChange(v as Period)}>
      <SelectTrigger aria-label={t("periodLabel")} className="shrink-0 text-[13px]">
        <SelectValue />
      </SelectTrigger>
      <SelectContent align="end">
        {options.map((p) => (
          <SelectItem key={p} value={p}>
            {t(PERIOD_LABEL[p])}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
