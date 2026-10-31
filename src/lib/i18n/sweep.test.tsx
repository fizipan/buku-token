import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import historyFixture from "../../test/fixtures/history-view.json";
import modelsFixture from "../../test/fixtures/models-view.json";
import overviewEn from "../../test/fixtures/overview-quota-en.json";
import overviewId from "../../test/fixtures/overview-quota.json";
import projectsFixture from "../../test/fixtures/projects-view.json";
import settingsFixture from "../../test/fixtures/settings.json";
import statusFixture from "../../test/fixtures/scan-status.json";
import sessionsEn from "../../test/fixtures/session-page-en.json";
import { stubChartLayout } from "../../test/layout";
import { commands, emit, invokeMock, listenMock, resetTauri } from "../../test/tauri";

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: [string, unknown?]) => invokeMock(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: Parameters<typeof listenMock>) => listenMock(...a) }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ onFocusChanged: async () => () => {} }) }));
vi.mock("@tauri-apps/plugin-autostart", () => ({ isEnabled: async () => false, enable: async () => {}, disable: async () => {} }));

import App from "../../App";
import { SharePanel } from "../../components/SharePanel";
import { Welcome } from "../../components/Welcome";
import { resetUsdToIdrCache } from "../../hooks/useUsdToIdr";
import { Settings } from "../../settings/Settings";
import { Mutasi } from "../../tabs/Mutasi";
import { Model } from "../../tabs/Model";
import { Project } from "../../tabs/Project";
import { Ringkasan } from "../../tabs/Ringkasan";
import { Riwayat } from "../../tabs/Riwayat";
import type { AgentQuota, Overview } from "../api";
import { I18nProvider } from ".";

/** Words that only the Indonesian UI uses. None may survive in English. */
const INDONESIAN = ["Sisa", "Terpakai", "Hari Ini", "Biaya", "Pengaturan", "Perbarui", "Bagikan", "Mingguan", "Sesi", "Tanpa", "Harga", "Belum", "Terhubung"];

/** Everything the user can read or hear: text, plus aria-label, title, alt and placeholder. */
function readable(root: ParentNode): string {
  const attrs = [...root.querySelectorAll("[aria-label],[title],[alt],[placeholder]")].flatMap((el) =>
    ["aria-label", "title", "alt", "placeholder"].map((a) => el.getAttribute(a) ?? ""),
  );
  return [(root as HTMLElement).textContent ?? "", ...attrs].join("\n");
}

function expectEnglishOnly(root: ParentNode) {
  const text = readable(root);
  for (const word of INDONESIAN) expect(text, `"${word}" is left in:\n${text}`).not.toContain(word);
}

const en = (ui: ReactElement) => render(<I18nProvider lang="en">{ui}</I18nProvider>);
const NOW = new Date("2026-10-30T10:19:00Z");

beforeEach(() => {
  resetTauri();
  resetUsdToIdrCache();
  localStorage.clear();
  commands.get_settings = () => ({ ...settingsFixture, language: "en" });
  commands.scan_status = () => statusFixture;
  stubChartLayout();
  // The fixture's windows reset on 30 Oct; the clock must stand before that.
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("English sweep: no Indonesian text is left", () => {
  it("Ringkasan, with every section, the burn chart, a stale and an expired window", async () => {
    const base = overviewEn as unknown as Overview;
    const late = (id: string, label: string, over: object) => ({ id, label, usedPct: 70, resetsAt: "2026-11-03T00:00:00Z", observedAt: "2026-10-30T09:00:00Z", expired: false, ...over });
    const cursor: AgentQuota = {
      agent: "cr",
      plan: "Pro",
      windows: [late("total", "Total", { observedAt: "2026-10-25T09:00:00Z" }), late("auto", "Auto", { expired: true })],
      binding: null,
      nearLimit: false,
      unavailableReason: null,
    };
    const failing: AgentQuota = { ...base.quotas[1], agent: "cx", unavailableReason: "Offline" };
    const o: Overview = { ...base, quotas: [base.quotas[0], failing, cursor] };
    commands.get_overview = () => o;
    localStorage.setItem("buku-token:ringkasan-open", JSON.stringify({ cc: true, cr: true, or: true }));
    const { container } = en(<Ringkasan />);
    await screen.findByRole("heading", { name: "Today By Hour" });
    expect(screen.getByText("5h Session")).toBeInTheDocument();
    expect(screen.getByText("Left 38%")).toBeInTheDocument();
    expectEnglishOnly(container);
    // Money and tokens are English too.
    expect(container.textContent).toContain("$15.25");
    expect(container.textContent).toContain("Rp248,575");
    expect(container.textContent).toContain("1.24M");
  });

  it("Ringkasan in the Used display, with the unavailable and retry rows", async () => {
    const base = overviewEn as unknown as Overview;
    const o: Overview = { ...base, openrouter: { data: null, unavailableReason: "OpenRouter Key Rejected, Paste Again In Settings" }, antigravity: { today: 0, last7Days: 0, lastActive: "2026-10-01T00:00:00Z", totalSessions: 1200, totalSteps: 9000 } };
    commands.get_overview = () => o;
    commands.get_settings = () => ({ ...settingsFixture, language: "en", quotaDisplay: "used" });
    const { container } = en(<Ringkasan />);
    await screen.findByRole("heading", { name: "Today By Hour" });
    await waitFor(() => expect(container.textContent).toContain("Used 62%"));
    expect(screen.getByText("Open Settings")).toBeInTheDocument();
    expect(screen.getByText("Try Again")).toBeInTheDocument();
    expect(container.textContent).toContain("Sessions: 1,200");
    expectEnglishOnly(container);
  });

  it("Mutasi, with an unpriced session and the older-days button", async () => {
    commands.get_sessions = () => sessionsEn;
    const { container } = en(<Mutasi />);
    await screen.findByText("Today · Fri, Oct 30");
    expect(screen.getByRole("button", { name: "Load 3 Earlier Days" })).toBeInTheDocument();
    expect(screen.getAllByText("No Price Yet").length).toBeGreaterThan(0);
    expect(screen.getByLabelText("Filter by agent")).toBeInTheDocument();
    expectEnglishOnly(container);
  });

  it("Model, with the mix bars and the cache note", async () => {
    commands.get_models = () => modelsFixture;
    const { container } = en(<Model />);
    await screen.findByRole("heading", { name: "Cost By Model" });
    expect(screen.getByRole("heading", { name: "Where Tokens Go" })).toBeInTheDocument();
    expect(container.textContent).toContain("Cache read takes 60% of tokens but only 20% of cost");
    expectEnglishOnly(container);
  });

  it("Project, with a folded row and the trend note", async () => {
    commands.get_projects = () => projectsFixture;
    const { container } = en(<Project />);
    await screen.findByRole("heading", { name: "Cost By Project" });
    expect(container.textContent).toContain("3 Other Projects");
    expect(container.textContent).toContain("up from 35% in the previous period");
    expect(container.textContent).toContain("4 sessions");
    expectEnglishOnly(container);
  });

  it("Riwayat, with the subscription value and its cycle", async () => {
    commands.get_history = () => historyFixture;
    const { container } = en(<Riwayat />);
    await screen.findByRole("heading", { name: "Daily Cost" });
    expect(screen.getByRole("heading", { name: "Subscription Value" })).toBeInTheDocument();
    expect(container.textContent).toContain("Cycle Oct 1–31 · Day 30");
    expect(container.textContent).toContain("Payback");
    expectEnglishOnly(container);
  });

  it("Welcome, with a found agent and with none", async () => {
    commands.scan_status = () => ({ ...statusFixture, sources: [{ agent: "cc", path: "/x", found: true }] });
    commands.save_settings = () => ({});
    const { container, unmount } = en(<Welcome onDone={() => {}} />);
    await screen.findByRole("list", { name: "Found On This Mac" });
    expect(screen.getByRole("button", { name: "Start" })).toBeInTheDocument();
    expect(container.textContent).toContain("Claude Code-credentials");
    expectEnglishOnly(container);
    unmount();
    commands.scan_status = () => ({ ...statusFixture, sources: [] });
    const again = en(<Welcome onDone={() => {}} />);
    expect(await screen.findByText("No Agents Detected Yet")).toBeInTheDocument();
    expectEnglishOnly(again.container);
  });

  it("Welcome keeps the privacy wording as exact as the Indonesian", async () => {
    commands.scan_status = () => ({ ...statusFixture, sources: [{ agent: "cc", path: "/Users/dev/.claude/projects", found: true }] });
    en(<Welcome onDone={() => {}} />);
    await screen.findByText(/Always Allow/); // the Keychain note comes with Claude Code
    const body = document.body.textContent ?? "";
    for (const must of ["only from Anthropic's usage API", "No new account and no tracking", "sent only to cursor.com", "only to openrouter.ai", "only after you connect them", "“Always Allow”"]) {
      expect(body, must).toContain(must);
    }
  });

  it("Settings, with every group, both forms and the plan form open", async () => {
    commands.get_settings = () => ({ ...settingsFixture, onboarded: true, language: "en" });
    commands.cursor_session_status = () => ({ connected: true, expiresAt: "2027-01-05T00:00:00Z", keychainDenied: false, lastReason: null, historyFailed: true });
    commands.openrouter_session_status = () => ({ connected: false, keychainDenied: false, lastReason: null });
    const user = userEvent.setup();
    const { container } = render(<Settings />);
    await screen.findByRole("heading", { name: "Subscriptions" });
    for (const group of ["General", "Display", "Agents", "Notifications", "Privacy", "Data"]) {
      expect(screen.getByRole("heading", { name: group })).toBeInTheDocument();
    }
    await user.click(screen.getByRole("button", { name: "Manage Cursor" }));
    await user.click(screen.getByRole("button", { name: "Manage OpenRouter" }));
    await user.click(screen.getByRole("button", { name: "Edit Plan" }));
    await user.click(screen.getByRole("button", { name: "View" }));
    expect(screen.getByText("Valid Until January 5, 2027")).toBeInTheDocument();
    expect(screen.getByText("Cursor History Failed To Sync")).toBeInTheDocument();
    expectEnglishOnly(container);
  });

  it("SharePanel", async () => {
    commands.share_preview = () => "data:image/png;base64,AAAA";
    const { container } = en(<SharePanel onClose={() => {}} />);
    await screen.findByRole("img", { name: "Usage preview, Last 30 Days" });
    expect(screen.getByRole("button", { name: "Copy Image" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save PNG" })).toBeInTheDocument();
    expect(screen.getByText("Hide Cost")).toBeInTheDocument();
    expectEnglishOnly(container);
  });

  it("the popover shell, its tab bar and status bar", async () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ height: 400 } as DOMRect);
    commands.get_settings = () => ({ ...settingsFixture, onboarded: true, language: "en", lastTab: "ringkasan" });
    commands.get_overview = () => overviewEn;
    commands.resize_popover = () => null;
    commands.scan_status = () => ({ ...statusFixture, running: false, lastScanAt: new Date().toISOString() });
    const { container } = render(<App />);
    const tabs = await screen.findAllByRole("tab");
    await waitFor(() => expect(tabs.map((t) => t.textContent)).toEqual(["Overview", "Sessions", "Models", "Projects", "History"]));
    expect(screen.getByRole("button", { name: "Refresh" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Settings" })).toBeInTheDocument();
    expect(await screen.findByText("Updated just now")).toBeInTheDocument();
    expectEnglishOnly(container);
  });
});

describe("a language switch applies live", () => {
  it("the popover words itself again when the settings change and data-updated arrives", async () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ height: 400 } as DOMRect);
    let language: "id" | "en" = "id";
    commands.get_settings = () => ({ ...settingsFixture, onboarded: true, language, lastTab: "ringkasan" });
    commands.get_overview = () => (language === "en" ? overviewEn : overviewId);
    commands.resize_popover = () => null;
    commands.scan_status = () => ({ ...statusFixture, running: false, lastScanAt: new Date().toISOString() });
    render(<App />);
    expect(await screen.findByRole("tab", { name: "Ringkasan" })).toBeInTheDocument();
    expect(await screen.findByText("Sisa 38%")).toBeInTheDocument();
    language = "en";
    act(() => emit("data-updated"));
    expect(await screen.findByRole("tab", { name: "Overview" })).toBeInTheDocument();
    expect(await screen.findByText("Left 38%")).toBeInTheDocument();
    expect(document.documentElement.lang).toBe("en");
    expectEnglishOnly(document.body);
  });

  it("the Settings window changes language the moment Bahasa is clicked, and saves it", async () => {
    commands.get_settings = () => ({ ...settingsFixture, onboarded: true });
    let stored: unknown;
    commands.save_settings = (a) => (stored = (a as { settings: unknown }).settings);
    const user = userEvent.setup();
    const { container } = render(<Settings />);
    await screen.findByRole("heading", { name: "Langganan" });
    await user.click(screen.getByRole("radio", { name: "English" }));
    expect(await screen.findByRole("heading", { name: "Subscriptions" })).toBeInTheDocument();
    expect(stored).toEqual({ language: "en" });
    expect(screen.getByRole("radio", { name: "English" })).toBeChecked();
    expect(screen.getByRole("radiogroup", { name: "Language" })).toBeInTheDocument();
    expectEnglishOnly(container);
  });
});
