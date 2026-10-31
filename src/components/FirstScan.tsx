import { Progress } from "@/components/ui/progress";
import { useT } from "@/lib/i18n";

export function FirstScan({ done, total }: { done: number; total: number }) {
  const t = useT();
  const pct = total > 0 ? Math.min(100, (done / total) * 100) : 0;
  return (
    <div className="flex flex-col gap-3 px-5 py-10">
      <p className="text-[13px] tabular-nums">
        {t("scanning", { done, total })}
      </p>
      <Progress value={pct} aria-label={t("scanningLabel")} />
    </div>
  );
}
