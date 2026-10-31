import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { Button } from "@/components/ui/button";
import { AgentTile } from "@/components/AgentTile";
import { ServiceTile } from "@/components/ServiceTile";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { AGENT_NAME, STACK_ORDER } from "@/lib/agents";
import {
  connectCursor,
  connectOpenRouter,
  cursorLastReason,
  cursorSessionStatus,
  disconnectCursor,
  disconnectOpenRouter,
  getSettings,
  openrouterSessionStatus,
  saveSettings,
  scanStatus,
  sendTestNotification,
  updatePricing,
  type AgentKey,
  type CursorSessionStatus,
  type OpenRouterSessionStatus,
  type Lang,
  type Plan,
  type QuotaDisplay,
  type ResetDisplay,
  type ScanStatus,
  type Settings as SettingsDto,
  type ThemePref,
} from "@/lib/api";
import { I18nProvider, useI18n, type Key } from "@/lib/i18n";
import { setTheme } from "@/lib/theme";
import { ConfirmButton } from "./ConfirmButton";
import { CursorForm } from "./CursorForm";
import { OpenRouterForm, type AccountBusy } from "./OpenRouterForm";

/** Settings written before the display preferences existed have no such fields. */
const withDefaults = (s: SettingsDto): SettingsDto => ({
  ...s,
  theme: s.theme ?? "system",
  quotaDisplay: s.quotaDisplay ?? "left",
  resetDisplay: s.resetDisplay ?? "clock",
  notifyLowQuota: s.notifyLowQuota ?? false,
  notifyProjectedExhaustion: s.notifyProjectedExhaustion ?? false,
  hideOnScreenShare: s.hideOnScreenShare ?? false,
  openrouterEnabled: s.openrouterEnabled ?? true,
  antigravityEnabled: s.antigravityEnabled ?? true,
  language: s.language ?? "id",
});

const num = (v: string) => (v.trim() === "" ? NaN : Number(v));

/** Validators return the dictionary key of the error, so it is worded in the language current when shown. */
type Validate = (v: string) => Key | null;
const rateError: Validate = (v) => (num(v) > 0 && Number.isFinite(num(v)) ? null : "rateError");
const nameError: Validate = (v) => (v.trim() === "" ? "nameError" : null);
const priceError: Validate = (v) => {
  const n = num(v);
  return !Number.isFinite(n) ? "priceNumberError" : n < 0 ? "priceNegativeError" : null;
};
const dayError: Validate = (v) => {
  const n = num(v);
  return !Number.isInteger(n) || n < 1 || n > 31 ? "dayError" : null;
};

const message = (e: unknown) => String(e instanceof Error ? e.message : e);

const NOT_CONNECTED: CursorSessionStatus = {
  connected: false,
  expiresAt: null,
  keychainDenied: false,
  lastReason: null,
  historyFailed: false,
};

const OR_NOT_CONNECTED: OpenRouterSessionStatus = { connected: false, keychainDenied: false, lastReason: null };

const hasLocalFolder = (status: ScanStatus | null, agent: AgentKey) => {
  // Cursor's usage comes from cursor.com; its local folder only maps conversations to projects.
  if (agent === "cr") return true;
  const own = status?.sources.filter((s) => s.agent === agent) ?? [];
  return own.length === 0 || own.some((s) => s.found);
};

function Group({ title, foot, as: Tag = "div", children }: { title: string; foot?: ReactNode; as?: "div" | "ul"; children: ReactNode }) {
  return (
    <section className="mt-[18px] first:mt-0">
      <h2 className="mb-[7px] ml-0.5 text-[12.5px] font-medium text-muted-foreground">{title}</h2>
      <Tag className="rounded-[11px] border bg-card [&>*+*]:border-t">{children}</Tag>
      {foot && <p className="mx-[3px] mt-1.5 text-xs leading-[1.4] text-muted-foreground">{foot}</p>}
    </section>
  );
}

function Row({ label, note, children }: { label: ReactNode; note?: ReactNode; children?: ReactNode }) {
  return (
    <div data-row className="flex min-h-11 items-center justify-between gap-3.5 px-[13px] py-2.5 text-[13.5px]">
      <span className="min-w-0">
        {label}
        {note && <small className="mt-px block text-xs text-muted-foreground">{note}</small>}
      </span>
      {children && <span className="flex shrink-0 items-center gap-2.5">{children}</span>}
    </div>
  );
}

function Seg<T extends string>({ label, value, options, onChange }: { label: string; value: T; options: [T, string][]; onChange: (v: T) => void }) {
  const move = (e: KeyboardEvent, i: number) => {
    const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const next = options[(i + step + options.length) % options.length][0];
    onChange(next);
    ((e.currentTarget as HTMLElement).parentElement?.children[options.findIndex((o) => o[0] === next)] as HTMLElement | undefined)?.focus();
  };
  return (
    <div role="radiogroup" aria-label={label} className="inline-grid grid-flow-col rounded-lg bg-muted p-0.5">
      {options.map(([v, text], i) => (
        <button
          key={v}
          type="button"
          role="radio"
          aria-checked={v === value}
          tabIndex={v === value ? 0 : -1}
          onClick={() => onChange(v)}
          onKeyDown={(e) => move(e, i)}
          className={`whitespace-nowrap rounded-md px-2.5 py-1 text-[12.5px] font-medium outline-none focus-visible:ring-[3px] focus-visible:ring-ring ${
            v === value ? "bg-background text-foreground shadow-sm" : "text-muted-foreground"
          }`}
        >
          {text}
        </button>
      ))}
    </div>
  );
}

/**
 * Keeps what is typed until blur or Enter. A valid value is committed; an invalid one shows its error and is not.
 * The error is tied to the input with `aria-describedby`. A parent that has a place for it (a row's note) takes it
 * through `onProblem` and renders an element with `errorId`; otherwise it is shown right under the input.
 */
function CommitInput({
  value,
  label,
  validate,
  onCommit,
  onValidity,
  onProblem,
  errorId,
  ...props
}: {
  value: string;
  label: string;
  validate: Validate;
  onCommit: (v: string) => void;
  /** Told whether the field currently holds an error, so a parent can refuse to close over it. */
  onValidity?: (invalid: boolean) => void;
  onProblem?: (error: Key | null) => void;
  errorId?: string;
} & Omit<React.ComponentProps<typeof Input>, "value" | "onChange" | "onBlur" | "onKeyDown">) {
  const { t } = useI18n();
  const ownErrorId = useId();
  const describedBy = errorId ?? ownErrorId;
  const [text, setText] = useState(value);
  const [error, setError] = useState<Key | null>(null);
  const report = useRef(onValidity);
  report.current = onValidity;
  const reportError = useRef(onProblem);
  reportError.current = onProblem;
  const show = (e: Key | null) => {
    setError(e);
    reportError.current?.(e);
  };
  useEffect(() => {
    setText(value);
    setError(null);
    reportError.current?.(null);
    report.current?.(false);
  }, [value]);
  useEffect(
    () => () => {
      report.current?.(false);
      reportError.current?.(null);
    },
    [],
  );
  const commit = () => {
    const e = validate(text);
    show(e);
    report.current?.(e !== null);
    if (!e && text !== value) onCommit(text);
  };
  return (
    <>
      <Input
        {...props}
        aria-label={label}
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? describedBy : undefined}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => e.key === "Enter" && commit()}
      />
      {error && !onProblem && (
        <p id={describedBy} role="alert" className="text-xs text-destructive">
          {t(error)}
        </p>
      )}
    </>
  );
}

function PlanForm({
  plan,
  onChange,
  onRemove,
  onValidity,
}: {
  plan: Plan;
  onChange: (patch: Partial<Plan>) => void;
  onRemove: () => void;
  onValidity: (field: string, invalid: boolean) => void;
}) {
  const { t } = useI18n();
  const id = `plan-${plan.id}`;
  return (
    <div role="group" aria-label={plan.name || t("planDefaultName")} className="flex flex-col gap-3 bg-muted/40 px-[13px] py-3">
      <div className="flex flex-col gap-1.5">
        <Label htmlFor={`${id}-name`} className="text-[13px]">
          {t("planName")}
        </Label>
        <CommitInput id={`${id}-name`} label={t("planName")} value={plan.name} validate={nameError} onValidity={(i) => onValidity("name", i)} onCommit={(v) => onChange({ name: v.trim() })} />
      </div>
      <div className="grid grid-cols-2 gap-3">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor={`${id}-price`} className="text-[13px]">
            {t("planPrice")}
          </Label>
          <CommitInput
            id={`${id}-price`}
            label={t("planPrice")}
            type="number"
            step="any"
            value={String(plan.priceUsd)}
            validate={priceError}
            onValidity={(i) => onValidity("price", i)}
            onCommit={(v) => onChange({ priceUsd: Number(v) })}
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor={`${id}-day`} className="text-[13px]">
            {t("planDay")}
          </Label>
          <CommitInput
            id={`${id}-day`}
            label={t("planDay")}
            type="number"
            min={1}
            max={31}
            value={String(plan.renewalDay)}
            validate={dayError}
            onValidity={(i) => onValidity("day", i)}
            onCommit={(v) => onChange({ renewalDay: Number(v) })}
          />
        </div>
      </div>
      <fieldset className="flex flex-col gap-2">
        <legend className="mb-1 text-[13px] font-medium">{t("planAgents")}</legend>
        <div className="flex flex-wrap gap-x-4 gap-y-2">
          {STACK_ORDER.map((a) => (
            <div key={a} className="flex items-center gap-2">
              <Checkbox
                id={`${id}-cov-${a}`}
                checked={plan.agents.includes(a)}
                onCheckedChange={(on) => onChange({ agents: on ? [...plan.agents, a] : plan.agents.filter((x) => x !== a) })}
              />
              <Label htmlFor={`${id}-cov-${a}`} className="text-[13px] font-normal">
                {AGENT_NAME[a]}
              </Label>
            </div>
          ))}
        </div>
      </fieldset>
      <div className="self-start text-destructive">
        <ConfirmButton
          label={t("remove")}
          ariaLabel={t("planRemoveAria", { name: plan.name || t("planDefaultName") })}
          question={t("confirmRemovePlan", { name: plan.name || t("planDefaultName") })}
          yes={t("yesRemove")}
          onConfirm={onRemove}
        />
      </div>
    </div>
  );
}

/** The language comes from the form's own state, so a click on Bahasa words the whole window at once. */
export function Settings() {
  const [lang, setLang] = useState<Lang>("id");
  return (
    <I18nProvider lang={lang}>
      <SettingsView onLanguage={setLang} />
    </I18nProvider>
  );
}

function SettingsView({ onLanguage }: { onLanguage: (lang: Lang) => void }) {
  const { t, f } = useI18n();
  const [settings, setSettings] = useState<SettingsDto | null>(null);
  const current = useRef<SettingsDto | null>(null);
  const [autostart, setAutostart] = useState(false);
  const [status, setStatus] = useState<ScanStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  // The version and the failure detail are kept, not the sentence: it is worded when shown, in the current language.
  const [pricing, setPricing] = useState<{ busy: boolean; version?: string; error?: string }>({ busy: false });
  const [openPlan, setOpenPlan] = useState<string | null>(null);
  const [foldersOpen, setFoldersOpen] = useState(false);
  const [testSent, setTestSent] = useState(false);

  // The forms keep the pasted cookie or key themselves, and empty it once it is sent.
  const [cursor, setCursor] = useState<CursorSessionStatus | null>(null);
  const [cursorOpen, setCursorOpen] = useState(false);
  // OpenRouter is not an agent: its own status, form and switch.
  const [openrouter, setOpenrouter] = useState<OpenRouterSessionStatus | null>(null);
  const [openrouterOpen, setOpenrouterOpen] = useState(false);
  // Kept here, not in the forms: closing a form while a request runs must not enable Putuskan again or lose the answer.
  const [cursorState, setCursorState] = useState<AccountBusy>({ busy: false });
  const [orState, setOrState] = useState<AccountBusy>({ busy: false });
  const [rateProblem, setRateProblem] = useState<Key | null>(null);
  // The plan whose form refused to close because a field has an error.
  const [blockedPlan, setBlockedPlan] = useState<string | null>(null);
  const addPlanButton = useRef<HTMLButtonElement>(null);
  const [focusAddPlan, setFocusAddPlan] = useState(0);
  useEffect(() => {
    if (focusAddPlan > 0) addPlanButton.current?.focus();
  }, [focusAddPlan]);

  const [attempt, setAttempt] = useState(0);
  // Saves run one after another, each built from the latest state, so none can overwrite a newer one.
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  // Saves not finished yet, in total and per key. A reply or a reload only replaces the screen when none is left.
  const inFlight = useRef(0);
  const keysInFlight = useRef(new Map<string, number>());
  const failedSave = useRef(false);
  const loginBusy = useRef(false);
  const invalidFields = useRef(new Set<string>());

  const apply = (s: SettingsDto) => {
    const next = withDefaults(s);
    current.current = next;
    setSettings(next);
  };

  // The window is hidden, not destroyed. "settings-opened" (sent by Rust right before it shows the
  // window) resets everything. A plain focus gain (Cmd-Tab back) only reloads the values. Cursor's
  // session status is read on open and after its own actions only, because reading it can touch the Keychain.
  useEffect(() => {
    let live = true;
    const fail = (e: unknown) => live && setLoadError(String(e));
    const load = (withCursor: boolean) =>
      Promise.all([
        getSettings(),
        scanStatus(),
        isEnabled().catch(() => null),
        withCursor ? cursorSessionStatus().catch(() => null) : Promise.resolve(null),
        withCursor ? openrouterSessionStatus().catch(() => null) : Promise.resolve(null),
      ])
        .then(([s, st, auto, cur, orStatus]) => {
          if (!live) return;
          if (withCursor) {
            setCursor(cur);
            setOpenrouter(orStatus);
          }
          if (inFlight.current === 0) {
            apply(s);
            setAutostart(auto ?? s.launchAtLogin);
          }
          setStatus(st);
          setSaveError(null);
          setPricing({ busy: false });
          setLoadError(null);
        })
        .catch(fail);
    void load(true);
    const unFocus = getCurrentWindow().onFocusChanged(({ payload }) => {
      if (payload) void load(false);
    });
    const unOpened = listen("settings-opened", () => {
      // A half-typed cookie or key goes with its form, which closes when the window is reopened, but is
      // kept across a plain focus gain.
      setCursorState((s) => (s.busy ? s : { busy: false }));
      setCursorOpen(false);
      setOpenrouterOpen(false);
      setOrState((s) => (s.busy ? s : { busy: false }));
      setOpenPlan(null);
      invalidFields.current.clear();
      setFoldersOpen(false);
      void load(true);
    });
    return () => {
      live = false;
      void unFocus.then((f) => f()).catch(() => {});
      void unOpened.then((f) => f()).catch(() => {});
    };
  }, [attempt]);

  const theme = settings?.theme;
  useEffect(() => {
    if (theme) setTheme(theme);
  }, [theme]);
  const language = settings?.language;
  useEffect(() => {
    if (language) onLanguage(language);
  }, [language, onLanguage]);

  if (!settings) {
    return (
      <div className="flex h-screen items-center justify-center bg-background px-6 text-[13px] text-muted-foreground">
        {loadError ? (
          <div role="alert" className="flex flex-col items-center gap-2">
            <p>{t("settingsLoadFailed", { error: loadError })}</p>
            <Button variant="outline" size="sm" onClick={() => setAttempt((n) => n + 1)}>
              {t("retry")}
            </Button>
          </div>
        ) : (
          <p>{t("loading")}</p>
        )}
      </div>
    );
  }

  // Reloads the stored settings, unless a save is in flight (its reply, or the next reload, wins). Used
  // after the first Cursor connect adds the Cursor Pro plan behind this window's back (saves send the whole
  // plan list, so the next one would delete it again) and after a failed save.
  const reloadSettings = () => getSettings().then((s) => inFlight.current === 0 && apply(s), () => {});

  /**
   * Shows the change at once and saves it; resolves to whether the save worked. The reply replaces the
   * screen only when no other save is waiting (it would not know about their edits). A failed save puts
   * back the keys it changed, except those a later edit is still waiting to change.
   */
  const commit = (patch: Partial<SettingsDto>): Promise<boolean> => {
    const before = current.current!;
    const keys = Object.keys(patch);
    apply({ ...before, ...patch });
    setSaveError(null);
    inFlight.current += 1;
    keys.forEach((k) => keysInFlight.current.set(k, (keysInFlight.current.get(k) ?? 0) + 1));
    const run = async () => {
      let ok = true;
      let next: unknown;
      try {
        next = await saveSettings(patch);
      } catch (e) {
        ok = false;
        failedSave.current = true;
        setSaveError(message(e));
      }
      inFlight.current -= 1;
      const stale = new Set<string>();
      keys.forEach((k) => {
        const left = (keysInFlight.current.get(k) ?? 1) - 1;
        keysInFlight.current.set(k, left);
        if (left > 0) stale.add(k);
      });
      if (!ok) {
        const back = Object.fromEntries(keys.filter((k) => !stale.has(k)).map((k) => [k, before[k as keyof SettingsDto]]));
        apply({ ...current.current!, ...back });
      } else if (inFlight.current === 0 && next && typeof next === "object" && "plans" in next) {
        apply(next as SettingsDto);
      }
      if (inFlight.current === 0 && failedSave.current) {
        // A rollback restores what an earlier edit left on screen, which may never have been saved. Once
        // nothing is waiting, the stored settings are the truth.
        failedSave.current = false;
        await reloadSettings();
      }
      return ok;
    };
    const result = queue.current.then(run);
    queue.current = result;
    return result;
  };

  /**
   * After a language change has been saved: the texts the backend worded in the old language go (an error is
   * not worded again, only dropped), and the reasons next to Terhubung are read again in the new one. Cursor's
   * comes from memory, so no Keychain prompt; OpenRouter's status read is the same one the window opens with.
   */
  const rewordBackendTexts = () => {
    setSaveError(null);
    setCursorState((s) => (s.busy ? s : { busy: false }));
    setOrState((s) => (s.busy ? s : { busy: false }));
    void cursorLastReason().then((lastReason) => setCursor((c) => (c ? { ...c, lastReason } : c)), () => {});
    void openrouterSessionStatus().then(setOpenrouter, () => {});
  };

  const setPlans = (plans: Plan[]) => commit({ plans });
  const editPlan = (id: string, patch: Partial<Plan>) =>
    setPlans(current.current!.plans.map((p) => (p.id === id ? { ...p, ...patch } : p)));

  const toggleLogin = async (on: boolean) => {
    if (loginBusy.current) return;
    loginBusy.current = true;
    const set = (v: boolean) => (v ? enable() : disable());
    try {
      const now = await isEnabled().catch(() => current.current!.launchAtLogin);
      if (on !== now) await set(on);
      setAutostart(on);
      if (!(await commit({ launchAtLogin: on }))) {
        // The OS changed but the setting did not stick: put the OS back so the two agree.
        if (on !== now) await set(now).catch(() => {});
        setAutostart(now);
      }
    } catch (e) {
      setSaveError(message(e));
    } finally {
      loginBusy.current = false;
    }
  };

  const refreshPricing = async () => {
    setPricing({ busy: true });
    try {
      const v = await updatePricing();
      setPricing({ busy: false, version: v });
      scanStatus().then(setStatus).catch(() => {});
    } catch (e) {
      setPricing({ busy: false, error: message(e) });
    }
  };

  const connect = async (cookie: string) => {
    setCursorState({ busy: true, action: "connect" });
    try {
      setCursor(await connectCursor(cookie));
      setCursorState({ busy: false });
      await reloadSettings();
      return true;
    } catch (e) {
      setCursorState({ busy: false, error: message(e) });
      return false;
    }
  };

  const connectOr = async (key: string) => {
    setOrState({ busy: true, action: "connect" });
    try {
      setOpenrouter(await connectOpenRouter(key));
      setOrState({ busy: false });
      return true;
    } catch (e) {
      setOrState({ busy: false, error: message(e) });
      return false;
    }
  };

  const disconnectOr = async () => {
    setOrState({ busy: true, action: "disconnect" });
    try {
      await disconnectOpenRouter();
      setOpenrouter(OR_NOT_CONNECTED);
      setOrState({ busy: false });
      return true;
    } catch (e) {
      setOrState({ busy: false, error: message(e) });
      return false;
    }
  };

  const disconnect = async () => {
    setCursorState({ busy: true, action: "disconnect" });
    try {
      await disconnectCursor();
      setCursor(NOT_CONNECTED);
      setCursorState({ busy: false });
      return true;
    } catch (e) {
      setCursorState({ busy: false, error: message(e) });
      return false;
    }
  };

  const agentStatus = (a: AgentKey): { text: string; good?: boolean } => {
    if (a === "cr") {
      if (cursor?.connected) return { text: t("statusConnected"), good: true };
      return { text: cursor?.keychainDenied ? t("statusDenied") : t("statusNotConnected") };
    }
    if (!hasLocalFolder(status, a)) return { text: t("statusNotFound") };
    return a === "cc" || a === "cx" ? { text: t("statusQuotaActive"), good: true } : { text: t("payAsYouGo") };
  };

  const orStatusText = openrouter?.connected
    ? { text: t("statusConnected"), good: true }
    : { text: openrouter?.keychainDenied ? t("statusDenied") : t("statusNotConnected"), good: false };

  const sources = status?.sources ?? [];
  const activity = status?.activitySources ?? [];
  return (
    <div className="h-screen overflow-y-auto bg-background px-5 pb-[22px] pt-[18px] text-sm text-foreground">
      {loadError && (
        <div role="alert" className="mb-4 flex items-center justify-between gap-3 rounded-lg border border-destructive/40 p-3 text-[13px] text-destructive">
          <span>{t("settingsLoadFailed", { error: loadError })}</span>
          <Button variant="outline" size="sm" onClick={() => setAttempt((n) => n + 1)}>
            {t("retry")}
          </Button>
        </div>
      )}
      {saveError && (
        <p role="alert" className="sticky top-0 z-10 mb-4 rounded-lg border border-destructive/40 bg-background p-3 text-[13px] text-destructive">
          {t("saveFailed", { error: saveError })}
        </p>
      )}

      <Group title={t("general")}>
        <Row label={t("openAtLogin")}>
          <Switch aria-label={t("openAtLogin")} checked={autostart} onCheckedChange={toggleLogin} />
        </Row>
        <Row label={t("theme")}>
          <Seg<ThemePref>
            label={t("theme")}
            value={settings.theme}
            options={[["system", t("themeSystem")], ["light", t("themeLight")], ["dark", t("themeDark")]]}
            onChange={(theme) => commit({ theme })}
          />
        </Row>
        <Row label={t("language")}>
          <Seg<Lang>
            label={t("language")}
            value={settings.language}
            options={[["id", t("langId")], ["en", t("langEn")]]}
            onChange={(language) => void commit({ language }).then((ok) => ok && rewordBackendTexts())}
          />
        </Row>
      </Group>

      <Group title={t("display")} foot={t("displayFoot")}>
        <Row label={t("showQuota")}>
          <Seg<QuotaDisplay>
            label={t("showQuota")}
            value={settings.quotaDisplay}
            options={[["left", t("showLeft")], ["used", t("showUsed")]]}
            onChange={(quotaDisplay) => commit({ quotaDisplay })}
          />
        </Row>
        <Row label={t("resetTime")}>
          <Seg<ResetDisplay>
            label={t("resetTime")}
            value={settings.resetDisplay}
            options={[["countdown", t("countdown")], ["clock", t("clockOption")]]}
            onChange={(resetDisplay) => commit({ resetDisplay })}
          />
        </Row>
        <Row
          label={t("rate")}
          note={
            rateProblem && (
              <span id="rate-error" role="alert" className="text-destructive">
                {t(rateProblem)}
              </span>
            )
          }
        >
          <CommitInput
            label={t("rate")}
            errorId="rate-error"
            onProblem={setRateProblem}
            type="number"
            step="any"
            className="h-7 w-[88px] px-[9px] text-right text-[13px] md:text-[13px]"
            value={String(settings.usdToIdr)}
            validate={rateError}
            onCommit={(v) => Number(v) !== current.current!.usdToIdr && commit({ usdToIdr: Number(v) })}
          />
        </Row>
      </Group>

      <Group title={t("agents")} as="ul">
        {STACK_ORDER.map((a) => {
          const st = agentStatus(a);
          return (
            <li key={a}>
              <Row
                label={
                  <span className="flex items-center gap-[9px]">
                    <AgentTile agent={a} size={26} />
                    {AGENT_NAME[a]}
                  </span>
                }
              >
                {a === "cr" && (
                  <Button variant="outline" size="sm" aria-label={t("manageCursor")} aria-expanded={cursorOpen} onClick={() => setCursorOpen((o) => !o)}>
                    {t("manage")}
                  </Button>
                )}
                <span className={`whitespace-nowrap text-[12.5px] ${st.good ? "text-[var(--ok)]" : "text-muted-foreground"}`}>{st.text}</span>
                <Switch
                  aria-label={AGENT_NAME[a]}
                  checked={settings.enabledAgents[a] ?? true}
                  // always the whole map: the backend replaces it, it does not merge key by key
                  onCheckedChange={(on) =>
                    commit({
                      enabledAgents: {
                        ...Object.fromEntries(STACK_ORDER.map((k) => [k, current.current!.enabledAgents[k] ?? true])),
                        [a]: on,
                      },
                    })
                  }
                />
              </Row>
              {a === "cr" && cursorOpen && (
                <div className="border-t">
                  <CursorForm status={cursor} state={cursorState} onConnect={connect} onDisconnect={disconnect} />
                </div>
              )}
            </li>
          );
        })}
        <li>
          <Row
            label={
              <span className="flex items-center gap-[9px]">
                <ServiceTile service="openrouter" size={26} />
                OpenRouter
              </span>
            }
          >
            <Button variant="outline" size="sm" aria-label={t("manageOpenRouter")} aria-expanded={openrouterOpen} onClick={() => setOpenrouterOpen((o) => !o)}>
              {t("manage")}
            </Button>
            <span className={`whitespace-nowrap text-[12.5px] ${orStatusText.good ? "text-[var(--ok)]" : "text-muted-foreground"}`}>{orStatusText.text}</span>
            <Switch aria-label="OpenRouter" checked={settings.openrouterEnabled} onCheckedChange={(on) => commit({ openrouterEnabled: on })} />
          </Row>
          {openrouterOpen && (
            <div className="border-t">
              <OpenRouterForm status={openrouter} state={orState} onConnect={connectOr} onDisconnect={disconnectOr} />
            </div>
          )}
        </li>
        <li>
          <Row
            label={
              <span className="flex items-center gap-[9px]">
                <ServiceTile service="antigravity" size={26} />
                Antigravity
              </span>
            }
          >
            <span className="whitespace-nowrap text-[12.5px] text-muted-foreground">{t("activityOnly")}</span>
            <Switch aria-label="Antigravity" checked={settings.antigravityEnabled} onCheckedChange={(on) => commit({ antigravityEnabled: on })} />
          </Row>
        </li>
      </Group>

      <Group title={t("plans")} foot={t("plansFoot")}>
        {settings.plans.map((p) => (
          <div key={p.id}>
            <Row
              label={p.name || t("planDefaultName")}
              note={
                <>
                  {`${t("planRenewal", { n: p.renewalDay })}${p.agents.length ? ` · ${p.agents.map((a) => AGENT_NAME[a]).join(", ")}` : ""}`}
                  {blockedPlan === p.id && (
                    <span id={`plan-${p.id}-blocked`} role="alert" className="mt-px block text-destructive">
                      {t("planFixFirst")}
                    </span>
                  )}
                </>
              }
            >
              <span className="font-medium">{t("perMonth", { price: `$${f.price(p.priceUsd)}` })}</span>
              <Button
                variant="outline"
                size="sm"
                aria-expanded={openPlan === p.id}
                aria-describedby={blockedPlan === p.id ? `plan-${p.id}-blocked` : undefined}
                aria-label={t("planToggle", { action: openPlan === p.id ? t("close") : t("edit"), name: p.name || t("planDefaultName") })}
                onClick={() => {
                  // an invalid field keeps its error on screen instead of vanishing with the form
                  if (invalidFields.current.size > 0) {
                    setBlockedPlan(p.id);
                    return;
                  }
                  setBlockedPlan(null);
                  setOpenPlan(openPlan === p.id ? null : p.id);
                }}
              >
                {openPlan === p.id ? t("close") : t("edit")}
              </Button>
            </Row>
            {openPlan === p.id && (
              <div className="border-t">
                <PlanForm
                  plan={p}
                  onChange={(patch) => editPlan(p.id, patch)}
                  onValidity={(field, invalid) => {
                    invalidFields.current[invalid ? "add" : "delete"](field);
                    if (invalidFields.current.size === 0) setBlockedPlan(null);
                  }}
                  onRemove={() => {
                    invalidFields.current.clear();
                    setBlockedPlan(null);
                    setOpenPlan(null);
                    setPlans(current.current!.plans.filter((x) => x.id !== p.id));
                    // The button that was pressed is gone with the form.
                    setFocusAddPlan((n) => n + 1);
                  }}
                />
              </div>
            )}
          </div>
        ))}
        <Row label={<span className="text-muted-foreground">{t("addPlan")}</span>}>
          <Button
            ref={addPlanButton}
            variant="outline"
            size="sm"
            onClick={() => {
              const plan: Plan = { id: `plan-${Date.now().toString(36)}`, name: t("planNewName"), priceUsd: 0, renewalDay: 1, agents: [] };
              setPlans([...current.current!.plans, plan]);
              setOpenPlan(plan.id);
            }}
          >
            {t("add")}
          </Button>
        </Row>
      </Group>

      <Group title={t("notifications")} foot={t("notifyHelp")}>
        <Row label={t("notifyLowQuota")}>
          <Switch aria-label={t("notifyLowQuota")} checked={settings.notifyLowQuota} onCheckedChange={(on) => commit({ notifyLowQuota: on })} />
        </Row>
        <Row label={t("notifyProjected")} note={t("notifyProjectedNote")}>
          <Switch aria-label={t("notifyProjected")} checked={settings.notifyProjectedExhaustion} onCheckedChange={(on) => commit({ notifyProjectedExhaustion: on })} />
        </Row>
        <Row label={t("testNotification")}>
          <span role="status" aria-label={t("testNotification")} className="text-xs text-muted-foreground">
            {testSent ? t("testSent") : ""}
          </span>
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              setTestSent(false);
              sendTestNotification().then(() => setTestSent(true), (e) => setSaveError(message(e)));
            }}
          >
            {t("sendTest")}
          </Button>
        </Row>
      </Group>

      <Group title={t("privacy")} foot={t("hideOnShareFoot")}>
        <Row label={t("hideOnShare")}>
          <Switch aria-label={t("hideOnShare")} checked={settings.hideOnScreenShare} onCheckedChange={(on) => commit({ hideOnScreenShare: on })} />
        </Row>
      </Group>

      <Group
        title={t("data")}
        foot={
          <>
            {pricing.version && <span className="block">{t("priceUpdated", { version: pricing.version })}</span>}
            {pricing.error && (
              <span role="alert" className="block text-destructive">
                {t("priceUpdateFailed", { error: pricing.error })}
              </span>
            )}
          </>
        }
      >
        <Row label={t("priceList")}>
          <span className="text-muted-foreground">{status?.pricingVersion ?? "–"}</span>
          <Button variant="outline" size="sm" onClick={refreshPricing} disabled={pricing.busy}>
            {pricing.busy ? t("updating") : t("updateList")}
          </Button>
        </Row>
        <Row label={t("foldersRead")}>
          <span className="text-muted-foreground">{t("folderCount", { n: sources.length + activity.length })}</span>
          <Button variant="outline" size="sm" aria-expanded={foldersOpen} onClick={() => setFoldersOpen((o) => !o)}>
            {foldersOpen ? t("hide") : t("view")}
          </Button>
        </Row>
        {foldersOpen && (
          <ul className="flex flex-col gap-1 px-[13px] py-2.5">
            {sources.map((s) => (
              <li key={`${s.agent}:${s.path}`} className="flex items-baseline gap-2 text-xs">
                <span className="shrink-0 text-muted-foreground">{AGENT_NAME[s.agent]}</span>
                <span className={`break-all ${s.found ? "" : "text-muted-foreground line-through"}`}>{s.path}</span>
                {!s.found && <span className="sr-only">{t("statusNotFound")}</span>}
              </li>
            ))}
            {activity.map((s) => (
              <li key={`${s.name}:${s.path}`} className="flex items-baseline gap-2 text-xs">
                <span className="shrink-0 text-muted-foreground">{s.name}</span>
                <span className={`break-all ${s.found ? "" : "text-muted-foreground line-through"}`}>{s.path}</span>
                {!s.found && <span className="sr-only">{t("statusNotFound")}</span>}
              </li>
            ))}
          </ul>
        )}
        {(status?.skipped ?? 0) > 0 && (
          <Row label={t("skippedRows")}>
            <span className="text-muted-foreground">{status?.skipped}</span>
          </Row>
        )}
      </Group>
    </div>
  );
}
