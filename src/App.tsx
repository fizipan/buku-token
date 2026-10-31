import { useEffect, useLayoutEffect, useRef, useState, type ComponentType } from "react";
import { LogoSprite } from "./components/Logo";
import { FirstScan } from "./components/FirstScan";
import { Welcome } from "./components/Welcome";
import { StatusBar } from "./components/StatusBar";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { TabActiveContext } from "./hooks/useTabData";
import { useDisplayPrefs } from "./hooks/useUsdToIdr";
import { I18nProvider, useT } from "./lib/i18n";
import { setTheme } from "./lib/theme";
import {
  getSettings,
  onDataUpdated,
  onScanProgress,
  openSettings,
  refresh,
  resizePopover,
  saveSettings,
  scanStatus,
  type ScanStatus,
} from "./lib/api";
import { Ringkasan } from "./tabs/Ringkasan";
import { Mutasi } from "./tabs/Mutasi";
import { Model } from "./tabs/Model";
import { Project } from "./tabs/Project";
import { Riwayat } from "./tabs/Riwayat";
import { TAB_IDS, TAB_LABEL, toTabId, type TabId } from "./lib/tabs";

export const TAB_VIEWS: Record<TabId, ComponentType> = {
  ringkasan: Ringkasan,
  mutasi: Mutasi,
  model: Model,
  project: Project,
  riwayat: Riwayat,
};

const firstScanRunning = (s: ScanStatus | null) => s !== null && s.running && s.lastScanAt === null;

/** The language comes from the settings, so a change in the Settings window shows here at once. */
export default function App() {
  const { language } = useDisplayPrefs();
  return (
    <I18nProvider lang={language}>
      <Popover />
    </I18nProvider>
  );
}

function Popover() {
  const t = useT();
  const rootRef = useRef<HTMLDivElement>(null);
  const [tab, setTab] = useState<TabId | null>(null);
  const [onboarded, setOnboarded] = useState(true);
  const [status, setStatus] = useState<ScanStatus | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const { theme } = useDisplayPrefs();
  useEffect(() => setTheme(theme), [theme]);

  useEffect(() => {
    let live = true;
    setStartError(null);
    const loadStatus = () =>
      scanStatus()
        .then((s) => live && setStatus(s))
        .catch(() => {});
    Promise.all([getSettings(), scanStatus()])
      .then(([cfg, s]) => {
        if (!live) return;
        setTab(toTabId(cfg.lastTab));
        setOnboarded(cfg.onboarded);
        setStatus(s);
      })
      .catch((e) => live && setStartError(String(e)));
    const unProgress = onScanProgress((p) => {
      setProgress(p);
      if (p.total > 0 && p.done >= p.total) loadStatus();
    });
    const unData = onDataUpdated(loadStatus);
    return () => {
      live = false;
      void unProgress.then((f) => f());
      void unData.then((f) => f());
    };
  }, [attempt]);

  // The window is transparent and sized to the popover: report our height after every layout change.
  const ready = status !== null && tab !== null;
  useLayoutEffect(() => {
    const el = rootRef.current;
    if (!el || !ready) return;
    let last = 0;
    const report = () => {
      const h = Math.ceil(el.getBoundingClientRect().height);
      if (h > 0 && h !== last) {
        last = h;
        resizePopover(h).catch(() => {});
      }
    };
    report();
    const ro = new ResizeObserver(report);
    ro.observe(el);
    return () => ro.disconnect();
  }, [ready]);

  // Park initial focus on the popover itself so no tab trigger shows a focus ring on open; Tab still reaches the triggers.
  useEffect(() => {
    if (ready) rootRef.current?.focus({ preventScroll: true });
  }, [ready]);

  const onTabChange = (v: string) => {
    const next = toTabId(v);
    setTab(next);
    saveSettings({ lastTab: next }).catch(() => {});
  };

  const onRefresh = () => {
    setRefreshing(true);
    refresh()
      .then(() => scanStatus().then(setStatus))
      .catch(() => {})
      .finally(() => setRefreshing(false));
  };

  const scanning = firstScanRunning(status);

  return (
    <div
      ref={rootRef}
      data-testid="popover-root"
      tabIndex={-1}
      className="flex outline-none max-h-[720px] flex-col rounded-[16px] border bg-background text-sm text-foreground shadow-[0_20px_40px_-12px_rgba(0,0,0,.45),0_6px_14px_-6px_rgba(0,0,0,.25)]"
    >
      <LogoSprite />
      {!ready ? (
        <div className="flex min-h-[200px] flex-col items-center justify-center gap-2 px-5 text-center text-[13px] text-muted-foreground">
          {startError ? (
            <div role="alert" className="flex flex-col items-center gap-2">
              <p>{t("loadFailed", { error: startError })}</p>
              <Button variant="outline" size="sm" onClick={() => setAttempt((n) => n + 1)}>
                {t("retry")}
              </Button>
            </div>
          ) : (
            <p>{t("loading")}</p>
          )}
        </div>
      ) : !onboarded ? (
        <Welcome onDone={() => setOnboarded(true)} />
      ) : scanning ? (
        <FirstScan done={progress?.done ?? status.done} total={progress?.total ?? status.total} />
      ) : (
        <Tabs value={tab} onValueChange={onTabChange} className="min-h-0 flex-1 gap-0">
          <div className="shrink-0 px-3 pt-3">
            <TabsList className="grid h-auto w-full grid-cols-5">
              {TAB_IDS.map((id) => (
                <TabsTrigger key={id} value={id} className="text-[12.5px]">
                  {t(TAB_LABEL[id])}
                </TabsTrigger>
              ))}
            </TabsList>
          </div>
          {TAB_IDS.map((id) => {
            const View = TAB_VIEWS[id];
            // Stay mounted so a switch never flashes "Memuat…" or resizes the window twice; a
            // hidden panel takes no room, so the popover height is the active panel's alone.
            return (
              <TabsContent key={id} value={id} forceMount hidden={id !== tab} className="min-h-0 overflow-y-auto">
                <TabActiveContext.Provider value={id === tab}>
                  <View />
                </TabActiveContext.Provider>
              </TabsContent>
            );
          })}
        </Tabs>
      )}
      {ready && onboarded && !scanning && (
        <StatusBar
          lastScanAt={status.lastScanAt}
          refreshing={refreshing}
          scanning={status.running}
          onRefresh={onRefresh}
          onOpenSettings={() => openSettings().catch(() => {})}
        />
      )}
    </div>
  );
}
