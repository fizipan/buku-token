import type { ReactNode } from "react";
import { Button } from "@/components/ui/button";
import type { TabData } from "../hooks/useTabData";
import { useT } from "../lib/i18n";

/** Renders tab data: "Memuat…" on first load, dimmed previous data on refetch, inline error with retry. */
export function DataView<T>({ state, children }: { state: TabData<T>; children: (data: T) => ReactNode }) {
  const t = useT();
  const { data, error, refreshing, reload } = state;
  if (data === null) {
    if (error) {
      return (
        <div role="alert" className="flex flex-col items-center gap-2 px-5 py-8 text-center text-[13px]">
          <p className="text-muted-foreground">{t("loadFailed", { error })}</p>
          <Button variant="outline" size="sm" onClick={reload}>
            {t("retry")}
          </Button>
        </div>
      );
    }
    return <p className="px-5 py-8 text-center text-[13px] text-muted-foreground">{t("loading")}</p>;
  }
  return (
    <div data-testid="data-view" style={{ opacity: refreshing ? 0.6 : 1 }} className="transition-opacity">
      {error && (
        <div role="alert" className="flex items-center justify-between gap-2 px-5 pt-3 text-[12.5px] text-destructive">
          <span>{t("updateFailed", { error })}</span>
          <Button variant="ghost" size="sm" onClick={reload}>
            {t("retry")}
          </Button>
        </div>
      )}
      {children(data)}
    </div>
  );
}
