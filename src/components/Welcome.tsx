import { KeyRound, Link2, Lock } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import logo from "@/assets/logo/icon.svg";
import { AgentTile } from "@/components/AgentTile";
import { Button } from "@/components/ui/button";
import { AGENT_NAME, STACK_ORDER } from "@/lib/agents";
import { refresh, saveSettings, scanStatus, type AgentKey } from "@/lib/api";
import { useT } from "@/lib/i18n";

function Note({ icon, title, children }: { icon: ReactNode; title: string; children: ReactNode }) {
  return (
    <div className="flex items-start gap-[11px] text-[13px] leading-[1.42]">
      <span aria-hidden className="mt-px shrink-0 text-muted-foreground">
        {icon}
      </span>
      <div>
        <b className="mb-px block font-semibold">{title}</b>
        <span className="text-muted-foreground">{children}</span>
      </div>
    </div>
  );
}

/**
 * First-run screen. "Mulai" saves `onboarded`, which lets the backend poll the Claude quota,
 * and then refreshes so the macOS Keychain dialog appears right after the user read this.
 */
export function Welcome({ onDone }: { onDone: () => void }) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // null until the scan status is read, so "Belum Ada Agent Terdeteksi" never flashes while loading
  const [found, setFound] = useState<AgentKey[] | null>(null);

  useEffect(() => {
    let live = true;
    scanStatus().then(
      (s) => live && setFound(STACK_ORDER.filter((a) => s.sources.some((src) => src.agent === a && src.found))),
      () => live && setFound([]),
    );
    return () => {
      live = false;
    };
  }, []);

  const start = async () => {
    setBusy(true);
    setError(null);
    try {
      await saveSettings({ onboarded: true });
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
      setBusy(false);
      return;
    }
    onDone(); // the app moves on right away; the Keychain dialog may block the refresh for a while
    refresh().catch(() => {});
  };

  return (
    <div className="px-5 pb-5 pt-[22px]">
      <img src={logo} alt="" width={44} height={44} className="mb-3 block" />
      <h1 className="m-0 text-[19px] font-semibold tracking-[-0.02em]">{t("welcomeTitle")}</h1>
      <p className="mt-1.5 text-[13.5px] leading-[1.45] text-muted-foreground">
        {t("welcomeLead")}
      </p>

      <h2 className="mb-2 mt-[18px] text-[12.5px] font-medium text-muted-foreground">{t("foundOnMac")}</h2>
      {found === null ? null : found.length === 0 ? (
        <p className="text-[13px] text-muted-foreground">{t("noAgentsFound")}</p>
      ) : (
        <ul aria-label={t("foundOnMac")} className="grid grid-cols-2 gap-2">
          {found.map((a) => (
            <li key={a} className="flex min-w-0 items-center gap-2 text-[13px] font-medium">
              <AgentTile agent={a} size={26} />
              <span className="whitespace-nowrap">{AGENT_NAME[a]}</span>
            </li>
          ))}
        </ul>
      )}

      <h2 className="mb-2 mt-[18px] text-[12.5px] font-medium text-muted-foreground">{t("beforeStart")}</h2>
      <div className="grid gap-3">
        <Note icon={<Lock size={18} strokeWidth={1.4} />} title={t("noteLocalTitle")}>
          {t("noteLocal")}
        </Note>
        {/* The Keychain prompt is for Claude Code's login, so it is only announced where Claude Code is. */}
        {found?.includes("cc") && (
          <Note icon={<KeyRound size={18} strokeWidth={1.4} />} title={t("notePermTitle")}>
            {t("notePerm")}
          </Note>
        )}
        <Note icon={<Link2 size={18} strokeWidth={1.4} />} title={t("noteConnectTitle")}>
          {t("noteConnect")}
        </Note>
      </div>

      {error && (
        <p role="alert" className="mt-4 text-[13px] text-destructive">
          {t("saveFailed", { error })}
        </p>
      )}
      <Button onClick={start} disabled={busy} className="mt-5 h-10 w-full rounded-[9px] text-sm font-semibold">
        {t("start")}
      </Button>
    </div>
  );
}
