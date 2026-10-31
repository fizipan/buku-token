import { X } from "lucide-react";
import { useContext, useEffect, useRef, useState } from "react";
import { AgentTile } from "@/components/AgentTile";
import { PERIOD_LABEL } from "@/components/PeriodSelect";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { TabActiveContext } from "@/hooks/useTabData";
import { AGENT_NAME, STACK_ORDER } from "@/lib/agents";
import { getSettings, onDataUpdated, shareCopy, sharePreview, shareSave, type AgentKey, type Period, type ShareRequest } from "@/lib/api";
import { useLang, useT, type Key } from "@/lib/i18n";

const ALL = "__all__";
const PERIODS: readonly { value: Period; label: Key }[] = [
  { value: "today", label: "today" },
  { value: "7d", label: "share7d" },
  { value: "30d", label: "share30d" },
];

/** The app's own theme (Sistem, Terang or Gelap), which is the `dark` class on <html>, not macOS alone. */
const isDark = () => document.documentElement.classList.contains("dark");

const buildRequest = (period: Period, agent: string, hideCost: boolean): ShareRequest => ({
  period,
  agent: agent === ALL ? null : (agent as AgentKey),
  hideCost,
  dark: isDark(),
});

interface SharePanelProps {
  onClose: () => void;
}

/** Share usage as an image: a preview of the card, what it shows, and copy or save. Replaces the tab's body. */
export function SharePanel({ onClose }: SharePanelProps) {
  const t = useT();
  const lang = useLang();
  const [period, setPeriod] = useState<Period>("30d");
  const [agent, setAgent] = useState<string>(ALL);
  const [hideCost, setHideCost] = useState(false);
  const [enabled, setEnabled] = useState<readonly AgentKey[]>(STACK_ORDER);
  const [preview, setPreview] = useState<{ src: string; period: Period } | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  // What was done, not the sentence: it is worded when shown, so a language change rewords it.
  const [status, setStatus] = useState<{ copied: true } | { savedTo: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dark, setDark] = useState(isDark);
  // Bumped by `data-updated` while this tab is showing, so the card and the agent list stay current.
  const [tick, setTick] = useState(0);

  const panel = useRef<HTMLDivElement>(null);
  const request = useRef(0);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  // The tabs stay mounted: a panel in another tab must not answer Escape.
  const tabActive = useContext(TabActiveContext);
  const activeRef = useRef(tabActive);
  activeRef.current = tabActive;

  const current = () => buildRequest(period, agent, hideCost);

  useEffect(() => {
    panel.current?.focus({ preventScroll: true });
  }, []);

  useEffect(() => {
    let live = true;
    getSettings().then(
      (s) => live && setEnabled(STACK_ORDER.filter((a) => s.enabledAgents[a] !== false)),
      () => {},
    );
    return () => {
      live = false;
    };
  }, [tick]);

  // An agent that gets switched off can no longer be the content: back to All Agents.
  useEffect(() => {
    setAgent((a) => (a === ALL || enabled.includes(a as AgentKey) ? a : ALL));
  }, [enabled]);

  // The theme can change while the panel is open.
  useEffect(() => {
    const watcher = new MutationObserver(() => setDark(isDark()));
    watcher.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
    return () => watcher.disconnect();
  }, []);

  const missedUpdate = useRef(false);
  useEffect(() => {
    const un = onDataUpdated(() => {
      if (activeRef.current) setTick((n) => n + 1);
      else missedUpdate.current = true;
    });
    return () => void un.then((f) => f());
  }, []);
  useEffect(() => {
    if (tabActive && missedUpdate.current) {
      missedUpdate.current = false;
      setTick((n) => n + 1);
    }
  }, [tabActive]);

  // A Select that closes on Escape marks the event handled; only an unhandled Escape closes the panel.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !e.defaultPrevented && activeRef.current) onCloseRef.current();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  // A new choice makes the last copy or save message and error old news.
  useEffect(() => {
    setStatus(null);
    setError(null);
  }, [period, agent, hideCost]);

  // The old preview stays until the new one arrives; only the latest request may land. The card is drawn
  // in the current language and theme, and again on `data-updated`, so a redraw keeps the status and errors.
  useEffect(() => {
    const mine = ++request.current;
    setLoading(true);
    sharePreview(buildRequest(period, agent, hideCost)).then(
      (src) => {
        if (mine !== request.current) return;
        setPreview({ src, period });
        setError(null);
        setLoading(false);
      },
      (e) => {
        if (mine !== request.current) return;
        setError(String(e));
        setLoading(false);
      },
    );
    return () => {
      request.current++;
    };
  }, [period, agent, hideCost, lang, dark, tick]);

  const act = async (run: () => Promise<NonNullable<typeof status>>) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    setStatus(null);
    try {
      setStatus(await run());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const copy = () =>
    act(async () => {
      await shareCopy(current());
      return { copied: true };
    });
  const save = () => act(async () => ({ savedTo: await shareSave(current()) }));

  return (
    <div
      ref={panel}
      role="region"
      aria-label={t("shareUsage")}
      tabIndex={-1}
      className="min-w-0 px-5 pb-5 pt-4 outline-none"
    >
      <div className="mb-3 flex items-center justify-between">
        <h2 className="text-[15px] font-semibold">{t("shareUsage")}</h2>
        <Button variant="ghost" size="icon-sm" aria-label={t("close")} onClick={onClose}>
          <X aria-hidden />
        </Button>
      </div>

      <div className="aspect-[360/280] w-full overflow-hidden rounded-[14px] bg-muted" aria-busy={loading}>
        {preview && (
          <img
            src={preview.src}
            alt={t("sharePreviewAlt", { period: t(PERIOD_LABEL[preview.period]) })}
            className={`size-full object-contain transition-opacity ${loading ? "opacity-60" : ""}`}
          />
        )}
      </div>

      <div className="mt-3.5 grid gap-[11px]">
        <div role="group" aria-label={t("periodLabel")} className="grid grid-cols-3 rounded-[9px] bg-muted p-[3px]">
          {PERIODS.map((p) => (
            <button
              key={p.value}
              type="button"
              aria-pressed={period === p.value}
              aria-disabled={busy || undefined}
              onClick={() => !busy && setPeriod(p.value)}
              className={`rounded-[7px] py-[5px] aria-disabled:opacity-60 text-[12.5px] font-medium outline-none focus-visible:ring-[3px] focus-visible:ring-ring ${
                period === p.value ? "bg-background text-foreground shadow-sm" : "text-muted-foreground"
              }`}
            >
              {t(p.label)}
            </button>
          ))}
        </div>

        <div className="flex items-center justify-between gap-2.5 text-[13px]">
          <span aria-hidden>{t("shareShow")}</span>
          <Select value={agent} onValueChange={setAgent} disabled={busy}>
            <SelectTrigger aria-label={t("shareShow")} className="min-w-0 w-[170px] text-[13px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent align="end">
              <SelectItem value={ALL}>{t("allAgents")}</SelectItem>
              {enabled.map((a) => (
                <SelectItem key={a} value={a}>
                  <AgentTile agent={a} size={18} />
                  {AGENT_NAME[a]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div className="flex items-center justify-between gap-2.5 text-[13px]">
          <Label htmlFor="share-hide-cost" className="text-[13px] font-normal">
            {t("hideCost")}
          </Label>
          <Switch id="share-hide-cost" checked={hideCost} onCheckedChange={setHideCost} disabled={busy} />
        </div>

        <div className="mt-0.5 grid grid-cols-2 gap-2">
          <Button aria-disabled={busy || undefined} onClick={() => void copy()}>
            {t("copyImage")}
          </Button>
          <Button variant="outline" aria-disabled={busy || undefined} onClick={() => void save()}>
            {t("savePng")}
          </Button>
        </div>

        <p role="status" aria-live="polite" className="min-h-4 text-center text-[12.5px] text-muted-foreground">
          {status && ("copied" in status ? t("copied") : t("savedTo", { name: status.savedTo }))}
        </p>
        {error && (
          <p role="alert" className="-mt-2 text-center text-[12.5px] text-destructive">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}
