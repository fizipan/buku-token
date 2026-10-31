import { useEffect, useRef, useState, type FormEvent } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { OpenRouterSessionStatus } from "@/lib/api";
import { useT } from "@/lib/i18n";
import { ConfirmButton } from "./ConfirmButton";

/** State of a connect or disconnect. It lives in Settings, so closing and reopening the form cannot reset it. */
export interface AccountBusy {
  busy: boolean;
  /** What is running, so the status line says the right thing. */
  action?: "connect" | "disconnect";
  error?: string;
}

/**
 * Akun OpenRouter. The pasted key lives only in this field's state and is emptied once it is sent.
 * Both callbacks resolve to whether it worked.
 */
export function OpenRouterForm({
  status,
  state,
  onConnect,
  onDisconnect,
}: {
  status: OpenRouterSessionStatus | null;
  state: AccountBusy;
  onConnect: (key: string) => Promise<boolean>;
  onDisconnect: () => Promise<boolean>;
}) {
  const t = useT();
  const [key, setKey] = useState("");
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
    if (state.busy || key.trim() === "") return;
    setParted(false);
    if (await onConnect(key)) {
      setKey("");
      setRepaste(false);
      setFocusTick((n) => n + 1);
    }
  };

  const disconnect = async () => {
    if (await onDisconnect()) {
      setRepaste(false);
      setKey("");
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
      <h3 className="text-[13px] font-medium">{t("openrouterAccount")}</h3>
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
          <div className="flex flex-col">{status.keychainDenied && <p className="text-xs text-muted-foreground">{t("openrouterRepasteHint")}</p>}</div>
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
            <li>{t("openrouterStep1")}</li>
            <li>{t("openrouterStep2")}</li>
            <li>{t("openrouterStep3")}</li>
          </ol>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="openrouter-key" className="text-[13px]">
              {t("apiKey")}
            </Label>
            <Input id="openrouter-key" type="password" autoComplete="off" value={key} onChange={(e) => setKey(e.target.value)} />
          </div>
          <div className="flex gap-2">
            <Button type="submit" size="sm" disabled={key.trim() === ""} aria-disabled={state.busy || undefined}>
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
                  setKey("");
                }}
              >
                {t("cancel")}
              </Button>
            )}
          </div>
          <p className="text-xs text-muted-foreground">{t("openrouterStored")}</p>
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
