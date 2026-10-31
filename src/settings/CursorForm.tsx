import { useEffect, useRef, useState, type FormEvent } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { CursorSessionStatus } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { ConfirmButton } from "./ConfirmButton";
import type { AccountBusy } from "./OpenRouterForm";

/**
 * Akun Cursor. The pasted cookie lives only in this field's state and is emptied once it is sent. The state of a
 * connect or disconnect lives in Settings, so closing and reopening the form cannot reset it. Both callbacks
 * resolve to whether it worked.
 */
export function CursorForm({
  status,
  state,
  onConnect,
  onDisconnect,
}: {
  status: CursorSessionStatus | null;
  state: AccountBusy;
  onConnect: (cookie: string) => Promise<boolean>;
  onDisconnect: () => Promise<boolean>;
}) {
  const { lang, t } = useI18n();
  const [cookie, setCookie] = useState("");
  const [repaste, setRepaste] = useState(false);
  const [parted, setParted] = useState(false);
  // The button that was pressed goes away when the answer arrives: focus moves to the status line instead.
  const statusLine = useRef<HTMLParagraphElement>(null);
  const [focusTick, setFocusTick] = useState(0);
  useEffect(() => {
    if (focusTick > 0) statusLine.current?.focus();
  }, [focusTick]);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (state.busy || cookie.trim() === "") return;
    setParted(false);
    if (await onConnect(cookie)) {
      setCookie("");
      setRepaste(false);
      setFocusTick((n) => n + 1);
    }
  };

  const disconnect = async () => {
    if (await onDisconnect()) {
      setRepaste(false);
      setCookie("");
      setParted(true);
      setFocusTick((n) => n + 1);
    }
  };

  const known = status?.connected || status?.keychainDenied;
  const line = state.busy
    ? t(state.action === "disconnect" ? "disconnecting" : "connecting")
    : status?.keychainDenied
      ? t("keychainDenied")
      : status?.connected
        ? t("accountConnected")
        : parted
          ? t("accountDisconnected")
          : "";
  return (
    <div className="flex flex-col gap-3 bg-muted/40 px-[13px] py-3">
      <h3 className="text-[13px] font-medium">{t("cursorAccount")}</h3>
      <p
        ref={statusLine}
        role="status"
        tabIndex={-1}
        className="rounded-sm text-[13px] font-medium outline-none focus-visible:ring-[3px] focus-visible:ring-ring empty:sr-only"
      >
        {line}
        {!state.busy && status?.connected && status.lastReason && <span className="ml-2 font-normal text-destructive">{status.lastReason}</span>}
      </p>
      {known && (
        <div className="flex items-center justify-between gap-3">
          <div className="flex flex-col">
            {status.keychainDenied && <p className="text-xs text-muted-foreground">{t("cursorRepasteHint")}</p>}
            {status.historyFailed && <p className="text-xs text-destructive">{t("cursorHistoryFailed")}</p>}
            {status.connected && status.expiresAt && (
              <p className="text-xs text-muted-foreground">
                {new Date(status.expiresAt).getTime() <= Date.now()
                  ? t("expired")
                  : t("validUntil", { date: new Date(status.expiresAt).toLocaleDateString(lang === "id" ? "id-ID" : "en-US", { day: "numeric", month: "long", year: "numeric" }) })}
              </p>
            )}
          </div>
          <div className="flex gap-2">
            {!repaste && (
              <Button variant="outline" size="sm" aria-disabled={state.busy || undefined} onClick={() => !state.busy && setRepaste(true)}>
                {t("repaste")}
              </Button>
            )}
            <ConfirmButton label={t("disconnect")} question={t("confirmDisconnect")} yes={t("yesDisconnect")} busy={state.busy} onConfirm={() => void disconnect()} />
          </div>
        </div>
      )}
      {(!known || repaste) && (
        <form onSubmit={submit} className="flex flex-col gap-3">
          <ol className="list-decimal pl-5 text-[13px] text-muted-foreground">
            <li>{t("cursorStep1")}</li>
            <li>{t("cursorStep2")}</li>
            <li>{t("cursorStep3")}</li>
          </ol>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="cursor-cookie" className="text-[13px]">
              {t("sessionCookie")}
            </Label>
            <Input id="cursor-cookie" type="password" autoComplete="off" value={cookie} onChange={(e) => setCookie(e.target.value)} />
          </div>
          <div className="flex gap-2">
            <Button type="submit" size="sm" disabled={cookie.trim() === ""} aria-disabled={state.busy || undefined}>
              {state.busy && state.action !== "disconnect" ? t("connecting") : t("connect")}
            </Button>
            {repaste && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                aria-disabled={state.busy || undefined}
                onClick={() => {
                  if (state.busy) return;
                  setRepaste(false);
                  setCookie("");
                }}
              >
                {t("cancel")}
              </Button>
            )}
          </div>
          <p className="text-xs text-muted-foreground">{t("cursorStored")}</p>
        </form>
      )}
      {state.error && (
        <p role="alert" className="text-xs text-destructive">
          {state.error}
        </p>
      )}
    </div>
  );
}
