import { useEffect, useState } from "react";
import { RefreshCw, Settings } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useT } from "../lib/i18n";
import { updatedLabel } from "../lib/relative-time";

interface Props {
  lastScanAt: string | null;
  /** Our own refresh is running: a second press would do nothing. */
  refreshing: boolean;
  /** A scan runs in the background: the icon turns, but Refresh stays available. */
  scanning?: boolean;
  onRefresh: () => void;
  onOpenSettings: () => void;
}

export function StatusBar({ lastScanAt, refreshing, scanning = false, onRefresh, onOpenSettings }: Props) {
  const t = useT();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(id);
  }, []);

  return (
    <div className="flex shrink-0 items-center justify-between border-t py-2.5 pr-2.5 pl-5 text-[12.5px] text-muted-foreground">
      <span className="inline-flex items-center gap-2 tabular-nums">
        <i
          aria-hidden
          className="size-1.5 rounded-full bg-agent-oc shadow-[0_0_0_3px_color-mix(in_oklab,var(--a-oc)_22%,transparent)]"
        />
        {updatedLabel(lastScanAt, t, now)}
      </span>
      <span className="flex gap-1">
        {/* aria-disabled, not disabled: the button that was pressed keeps keyboard focus while it works. */}
        <Button variant="ghost" size="icon" aria-label={t("refresh")} aria-disabled={refreshing || undefined} onClick={() => !refreshing && onRefresh()}>
          <RefreshCw className={refreshing || scanning ? "motion-safe:animate-spin" : undefined} />
        </Button>
        <Button variant="ghost" size="icon" aria-label={t("settings")} onClick={onOpenSettings}>
          <Settings />
        </Button>
      </span>
    </div>
  );
}
