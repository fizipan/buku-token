import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import settingsFixture from "../test/fixtures/settings.json";
import quotaFixture from "../test/fixtures/overview-quota.json";
import { stubChartLayout } from "../test/layout";
import { commands, invokeMock, listenMock, resetTauri } from "../test/tauri";
import type { AgentQuota, OpenRouterData, Overview, OpenRouterView, QuotaWindow } from "../lib/api";
import { clock, shortDate } from "../lib/format";

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: [string, unknown?]) => invokeMock(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: Parameters<typeof listenMock>) => listenMock(...a) }));

import { resetUsdToIdrCache } from "../hooks/useUsdToIdr";
import { Ringkasan } from "./Ringkasan";

const NOW = new Date("2026-10-30T10:19:00Z");
const at = (min: number) => new Date(NOW.getTime() + min * 60_000).toISOString();
const STORAGE_KEY = "buku-token:ringkasan-open";

const win = (id: string, label: string, usedPct: number, over: Partial<QuotaWindow> = {}): QuotaWindow => ({
  id,
  label,
  usedPct,
  resetsAt: at(141),
  observedAt: at(-5),
  expired: false,
  ...over,
});

const quota = (agent: AgentQuota["agent"], windows: QuotaWindow[], over: Partial<AgentQuota> = {}): AgentQuota => ({
  agent,
  plan: null,
  windows,
  binding: windows[0] ?? null,
  nearLimit: false,
  unavailableReason: null,
  ...over,
});

const ccQuota = () =>
  quota("cc", [win("five_hour", "Sesi 5 jam", 38), win("seven_day", "Mingguan", 30, { resetsAt: at(3.5 * 1440) })], { plan: "Max 5x" });
const cxQuota = (windows?: QuotaWindow[], over: Partial<AgentQuota> = {}) =>
  quota("cx", windows ?? [win("five_hour", "Sesi 5 jam", 20), win("seven_day", "Mingguan", 45, { resetsAt: at(3 * 1440), observedAt: "2026-10-15T08:00:00Z" })], {
    plan: "Plus",
    binding: null,
    ...over,
  });
const crQuota = (over: Partial<AgentQuota> = {}) =>
  quota(
    "cr",
    [
      win("total", "Total", 75, { resetsAt: at(5 * 1440) }),
      win("auto", "Auto", 70, { resetsAt: at(5 * 1440) }),
      win("api", "API", 90, { resetsAt: at(5 * 1440) }),
    ],
    { plan: "Pro", ...over },
  );

type Costs = [number, number, number, number, number];
function overview(
  over: { quotas?: AgentQuota[]; byAgent?: Costs; unpriced?: Costs; today?: Partial<Overview["today"]> } & Omit<Partial<Overview>, "today"> = {},
): Overview {
  const { quotas, byAgent, unpriced, today, ...rest } = over;
  const base = quotaFixture as unknown as Overview;
  return {
    ...base,
    quotas: quotas ?? [ccQuota()],
    // The shared fixture carries both; only the tests of the two rows turn them on.
    openrouter: null,
    antigravity: null,
    ...rest,
    today: { ...base.today, byAgent: byAgent ?? [12.8, 0, 0, 0, 0], unpricedByAgent: unpriced ?? [0, 0, 0, 0, 0], ...today },
  };
}

async function show(o: Overview) {
  commands.get_overview = () => o;
  const view = render(<Ringkasan />);
  await screen.findByRole("heading", { name: "Hari Ini per Jam" });
  return view;
}

const sections = () => [...document.querySelectorAll<HTMLElement>("[data-agent]")];
const section = (agent: string) => sections().find((s) => s.dataset.agent === agent)!;
const toggle = (agent: string) => within(section(agent)).getByRole("button");
const legendItems = () => within(screen.getByTestId("summary-legend")).queryAllByRole("listitem");
const rowTexts = (agent: string) => within(section(agent)).queryAllByTestId("quota-row").map((r) => r.textContent ?? "");

beforeEach(() => {
  resetTauri();
  resetUsdToIdrCache();
  localStorage.clear();
  commands.get_settings = () => settingsFixture;
  stubChartLayout();
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("Ringkasan sections", () => {
  it("one section per agent with a quota, Claude, Codex, Cursor in that order", async () => {
    await show(overview({ quotas: [crQuota(), cxQuota(), ccQuota()], byAgent: [12.8, 3.1, 0.42, 0.36, 1.72] }));
    expect(sections().map((s) => s.dataset.agent)).toEqual(["cc", "cx", "cr"]);
  });

  it("agents without a quota get no section, only a place in the summary list", async () => {
    await show(overview({ byAgent: [12.8, 0, 0, 0.36, 0] }));
    expect(sections().map((s) => s.dataset.agent)).toEqual(["cc"]);
    expect(screen.queryByText("Per Pakai")).not.toBeInTheDocument();
    expect(legendItems().map((li) => li.textContent)).toEqual(["Claude Code$12,80", "Hermes$0,36"]);
  });

  it("nothing at all: no sections, the empty message stays", async () => {
    const o = overview({ quotas: [], byAgent: [0, 0, 0, 0, 0] });
    o.today = { ...o.today, costUsd: 0, tokens: 0, unpricedTokens: 0, hours: o.today.hours.map((h) => ({ ...h, byAgent: [0, 0, 0, 0, 0] })) };
    await show(o);
    expect(sections()).toHaveLength(0);
    expect(screen.getByText("Belum ada sesi hari ini")).toBeInTheDocument();
  });

  it("the hero number and the weekly block are gone", async () => {
    await show(overview());
    expect(screen.queryByTestId("hero-value")).not.toBeInTheDocument();
    expect(screen.queryByText(/Sisa kuota/)).not.toBeInTheDocument();
  });

  it("header: name and plan badge, no cost, chevron at the far end", async () => {
    await show(overview());
    const head = toggle("cc");
    expect(head).toHaveTextContent("Claude Code");
    expect(head).toHaveTextContent("Max 5x");
    expect(head).not.toHaveTextContent("$");
    expect(head).not.toHaveTextContent("Hari Ini");
    const end = head.lastElementChild!;
    expect(end.className).toContain("ml-auto");
    expect(end.lastElementChild?.tagName.toLowerCase()).toBe("svg");
    expect(end.lastElementChild).toHaveAttribute("aria-hidden", "true");
  });

  it("windows with different resets keep four columns and no 'Reset' in the header", async () => {
    await show(overview());
    expect(toggle("cc")).not.toHaveTextContent("Reset");
    expect(within(section("cc")).getAllByTestId("quota-row")[0].className).toContain("62px]");
  });
});

describe("Ringkasan display preferences", () => {
  it("Terpakai and Hitung Mundur from the settings reach the quota rows", async () => {
    commands.get_settings = () => ({ ...settingsFixture, quotaDisplay: "used", resetDisplay: "countdown" });
    await show(overview());
    await vi.waitFor(() => expect(rowTexts("cc")[0]).toContain("Terpakai 38%"));
    expect(rowTexts("cc")[0]).toContain("2j 21m");
    expect(rowTexts("cc")[0]).not.toContain("Sisa");
  });
});

describe("Ringkasan Claude", () => {
  it("open by default: every window, the pace tick on the weekly one and the burn chart", async () => {
    await show(overview());
    expect(toggle("cc")).toHaveAttribute("aria-expanded", "true");
    const rows = rowTexts("cc");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toContain("Sesi 5 jam");
    expect(rows[0]).toContain("Sisa 62%");
    expect(rows[0]).toContain(clock(at(141)));
    expect(rows[1]).toContain("Sisa 70%");
    expect(rows[1]).toContain(shortDate(at(3.5 * 1440)));
    const ticks = within(section("cc")).getAllByTestId("pace-tick");
    expect(ticks).toHaveLength(1);
    expect(ticks[0]).toHaveStyle({ left: "calc(50% - 1px)" });
    expect(within(section("cc")).getByText("Sekarang")).toBeInTheDocument();
  });

  it("closed: only the binding window, no chart", async () => {
    await show(overview());
    await userEvent.setup().click(toggle("cc"));
    expect(toggle("cc")).toHaveAttribute("aria-expanded", "false");
    expect(rowTexts("cc")).toHaveLength(1);
    expect(rowTexts("cc")[0]).toContain("Sesi 5 jam");
    expect(within(section("cc")).queryByText("Sekarang")).not.toBeInTheDocument();
  });

  it("closed without a binding window falls back to the first window", async () => {
    const q = { ...ccQuota(), binding: null };
    await show(overview({ quotas: [q] }));
    await userEvent.setup().click(toggle("cc"));
    expect(rowTexts("cc")).toHaveLength(1);
    expect(rowTexts("cc")[0]).toContain("Sesi 5 jam");
  });

  it("no burn chart while the 5 hour window is past its reset", async () => {
    const q = quota("cc", [win("five_hour", "Sesi 5 jam", 38, { resetsAt: at(-10), expired: true }), win("seven_day", "Mingguan", 30)]);
    await show(overview({ quotas: [q] }));
    expect(within(section("cc")).queryByText("Sekarang")).not.toBeInTheDocument();
  });

  it("red remaining share at 15% or less", async () => {
    const q = quota("cc", [win("five_hour", "Sesi 5 jam", 88)]);
    await show(overview({ quotas: [q] }));
    expect(within(section("cc")).getByText("Sisa 12%").className).toMatch(/destructive/);
  });
});

describe("Ringkasan Codex", () => {
  it("closed: the binding row; open: both windows with the stale one dated in red", async () => {
    const stale = win("seven_day", "Mingguan", 45, { resetsAt: at(3 * 1440), observedAt: "2026-10-15T08:00:00Z" });
    await show(overview({ quotas: [ccQuota(), cxQuota([stale, win("five_hour", "Sesi 5 jam", 20)], { binding: stale })], byAgent: [12.8, 3.1, 0, 0, 0] }));
    expect(toggle("cx")).toHaveAttribute("aria-expanded", "false");
    expect(rowTexts("cx")).toHaveLength(1);
    const date = within(section("cx")).getByTitle("Data Terakhir 15 Okt");
    expect(date).toHaveTextContent("Data Terakhir 15 Okt");
    expect(date.className).toMatch(/destructive/);
    expect(date).toHaveAttribute("title", "Data Terakhir 15 Okt");
    expect(within(section("cx")).queryByTestId("pace-tick")).not.toBeInTheDocument();

    await userEvent.setup().click(toggle("cx"));
    expect(rowTexts("cx")).toHaveLength(2);
    expect(rowTexts("cx")[1]).toContain("Sisa 80%");
  });

  it("a window past its reset: empty bar, the sentence and the data date, no old percentage", async () => {
    const week = win("seven_day", "Mingguan", 40, { resetsAt: at(-60), observedAt: "2026-10-15T08:00:00Z", expired: true });
    await show(overview({ quotas: [cxQuota([week])], byAgent: [0, 3.1, 0, 0, 0] }));
    const row = within(section("cx")).getByTestId("quota-row");
    expect(within(row).getByText("Sudah reset · belum ada data baru")).toBeInTheDocument();
    expect(within(row).getByText("15 Okt")).toBeInTheDocument();
    expect(within(row).queryByText(/Sisa/)).not.toBeInTheDocument();
    expect(within(row).queryByText(/60/)).not.toBeInTheDocument();
    expect(within(row).getByRole("progressbar").querySelector("[data-slot=progress-indicator]")).toHaveStyle({ transform: "translateX(-100%)" });
  });

  it("a window whose reset passed after it was read counts as past its reset", async () => {
    const week = win("seven_day", "Mingguan", 40, { resetsAt: at(-1), observedAt: at(-120) });
    await show(overview({ quotas: [cxQuota([week])], byAgent: [0, 3.1, 0, 0, 0] }));
    expect(within(section("cx")).getByText("Sudah reset · belum ada data baru")).toBeInTheDocument();
  });
});

describe("Ringkasan Cursor", () => {
  it("closed: Total only; open: Total, Auto and API without a pace tick", async () => {
    await show(overview({ quotas: [ccQuota(), crQuota()], byAgent: [12.8, 0, 0, 0, 1.72] }));
    expect(rowTexts("cr")).toHaveLength(1);
    expect(rowTexts("cr")[0]).toContain("Total");
    expect(rowTexts("cr")[0]).toContain("Sisa 25%");
    expect(toggle("cr")).toHaveTextContent(`Reset ${shortDate(at(5 * 1440))}`);
    await userEvent.setup().click(toggle("cr"));
    const rows = rowTexts("cr");
    expect(rows.map((r) => r.split("Sisa")[0])).toEqual(["Total", "Auto", "API"]);
    expect(rows[2]).toContain("Sisa 10%");
    expect(toggle("cr")).toHaveTextContent(`Reset ${shortDate(at(5 * 1440))}`);
    for (const r of rows) expect(r).not.toContain(shortDate(at(5 * 1440)));
    expect(within(section("cr")).getAllByTestId("quota-row")[0].className).not.toContain("62px]");
    expect(within(section("cr")).queryByTestId("pace-tick")).not.toBeInTheDocument();
  });

  it("the shared reset in the header counts down too under Hitung Mundur", async () => {
    commands.get_settings = () => ({ ...settingsFixture, resetDisplay: "countdown" });
    await show(overview({ quotas: [ccQuota(), crQuota()], byAgent: [12.8, 0, 0, 0, 1.72] }));
    await vi.waitFor(() => expect(toggle("cr")).toHaveTextContent("Reset 5 hari lagi"));
    expect(toggle("cr")).not.toHaveTextContent(shortDate(at(5 * 1440)));
  });

  it("resets a few seconds apart in the same minute still count as shared", async () => {
    const q = crQuota({ windows: [win("total", "Total", 75, { resetsAt: "2026-11-04T10:19:05Z" }), win("auto", "Auto", 70, { resetsAt: "2026-11-04T10:19:40Z" })] });
    await show(overview({ quotas: [q], byAgent: [0, 0, 0, 0, 1.72] }));
    expect(toggle("cr")).toHaveTextContent("Reset ");
  });

  it("resets in different minutes: no shared 'Reset' note", async () => {
    const q = crQuota({ windows: [win("total", "Total", 75, { resetsAt: "2026-11-04T10:19:05Z" }), win("auto", "Auto", 70, { resetsAt: "2026-11-04T10:21:00Z" })] });
    await show(overview({ quotas: [q], byAgent: [0, 0, 0, 0, 1.72] }));
    expect(toggle("cr")).not.toHaveTextContent("Reset");
  });

  it("opens by default when it is the only quota section", async () => {
    await show(overview({ quotas: [crQuota()], byAgent: [0, 0, 0, 0, 1.72] }));
    expect(toggle("cr")).toHaveAttribute("aria-expanded", "true");
    expect(rowTexts("cr")).toHaveLength(3);
  });

  it("unavailable: one row with the reason and a button to Settings, not a retry", async () => {
    let refreshed = 0;
    let opened = 0;
    commands.refresh = () => void refreshed++;
    commands.open_settings = () => void opened++;
    const reason = "Sesi Cursor Habis, Tempel Ulang Di Pengaturan";
    await show(overview({ quotas: [ccQuota(), crQuota({ windows: [], binding: null, plan: null, unavailableReason: reason })] }));
    const row = within(section("cr")).getByTestId("quota-reason-row");
    expect(within(row).getByText(reason)).toBeInTheDocument();
    expect(within(section("cr")).queryByText("Cursor Total")).not.toBeInTheDocument();
    expect(within(section("cr")).queryByRole("button", { expanded: false })).not.toBeInTheDocument();
    expect(within(row).queryByRole("button", { name: "Coba Lagi" })).not.toBeInTheDocument();
    await userEvent.setup().click(within(row).getByRole("button", { name: "Buka Pengaturan" }));
    expect(opened).toBe(1);
    expect(refreshed).toBe(0);
  });

  it("other agents keep the retry that refreshes", async () => {
    let refreshed = 0;
    commands.refresh = () => void refreshed++;
    await show(overview({ quotas: [quota("cc", [], { binding: null, plan: null, unavailableReason: "Sedang Offline" })] }));
    await userEvent.setup().click(screen.getByRole("button", { name: "Coba Lagi" }));
    expect(refreshed).toBe(1);
  });
});

describe("Ringkasan summary", () => {
  const all = (): Costs => [12.8, 3.1, 0.36, 0.42, 1.72];
  const total = { costUsd: 18.4, tokens: 1_840_000 };

  it("donut centre shows the total, the list shows each agent with a cost, highest cost first", async () => {
    await show(overview({ quotas: [ccQuota(), cxQuota(), crQuota()], byAgent: all(), today: total }));
    const donut = screen.getByRole("img", { name: /\$18,40/ });
    expect(donut).toHaveTextContent("$18,40");
    expect(donut).toHaveTextContent("Hari Ini");
    expect(legendItems().map((li) => li.textContent)).toEqual(["Claude Code$12,80", "Codex$3,10", "Cursor$1,72", "Hermes$0,42", "OpenCode$0,36"]);
  });

  it("the donut segments follow the same order and the unpriced agent goes last", async () => {
    vi.stubGlobal("matchMedia", (query: string) => ({ matches: true, media: query, addEventListener() {}, removeEventListener() {} }));
    await show(overview({ byAgent: [1, 0, 5, 0, 3], unpriced: [0, 900, 0, 0, 0], today: { costUsd: 9 } }));
    expect(legendItems().map((li) => li.querySelector("span")?.textContent)).toEqual(["OpenCode", "Cursor", "Claude Code", "Codex"]);
    const fills = [...screen.getByRole("img", { name: /\$9,00/ }).querySelectorAll(".recharts-pie-sector path")].map((p) => p.getAttribute("fill"));
    expect(fills).toEqual(["var(--a-oc)", "var(--a-cr)", "var(--a-cc)"]);
  });

  it("an agent without cost is not listed and gets no segment", async () => {
    await show(overview({ byAgent: [12.8, 0, 0, 0, 1.72], today: { costUsd: 14.52 } }));
    expect(legendItems().map((li) => li.textContent)).toEqual(["Claude Code$12,80", "Cursor$1,72"]);
    expect(screen.getByRole("img", { name: /\$14,52/ }).querySelectorAll(".recharts-pie-sector")).toHaveLength(2);
  });

  it("a single agent still gets a list and a donut", async () => {
    await show(overview({ today: { costUsd: 12.8 } }));
    expect(legendItems()).toHaveLength(1);
    expect(screen.getByRole("img", { name: /\$12,80/ }).querySelectorAll(".recharts-pie-sector")).toHaveLength(1);
  });

  it("Rupiah and token totals sit in the two-column box", async () => {
    await show(overview({ byAgent: all(), today: total }));
    const box = screen.getByTestId("summary-stats");
    expect(box).toHaveTextContent("Dalam Rupiah");
    expect(box).toHaveTextContent(/Rp\s?\d/);
    expect(box).toHaveTextContent("Total Token");
    expect(box).toHaveTextContent("1,84 jt");
  });

  it("an agent with only unpriced tokens is listed with 'Harga Belum Ada', not $0,00", async () => {
    await show(overview({ byAgent: [12.8, 0, 0, 0, 0], unpriced: [0, 0, 5000, 0, 0], today: { costUsd: 12.8 } }));
    const items = legendItems();
    expect(items).toHaveLength(2);
    expect(items[1]).toHaveTextContent("OpenCode");
    expect(items[1]).toHaveTextContent("Harga Belum Ada");
    expect(items[1]).not.toHaveTextContent("$0,00");
  });

  it("all cost unpriced: dashes in the donut centre and Rupiah, an empty ring, never zero", async () => {
    await show(overview({ quotas: [], byAgent: [0, 0, 0, 0, 0], unpriced: [0, 0, 5000, 0, 0], today: { costUsd: 0, tokens: 5000, unpricedTokens: 5000 } }));
    expect(screen.getByRole("img", { name: /Harga Belum Ada/ })).toHaveTextContent("–");
    expect(screen.getByTestId("summary-stats")).toHaveTextContent("Dalam Rupiah–");
    expect(screen.getByTestId("summary-stats")).toHaveTextContent("5 rb");
    expect(screen.queryByText(/\$0,00/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Rp0/)).not.toBeInTheDocument();
    expect(legendItems()).toHaveLength(1);
  });

  it("no usage at all: an empty ring in --muted and no list", async () => {
    // Reduced motion draws the ring at once; otherwise Recharts animates it from nothing.
    vi.stubGlobal("matchMedia", (query: string) => ({ matches: true, media: query, addEventListener() {}, removeEventListener() {} }));
    await show(overview({ quotas: [], byAgent: [0, 0, 0, 0, 0], today: { costUsd: 0, tokens: 0, unpricedTokens: 0 } }));
    const donut = screen.getByRole("img", { name: /\$0,00/ });
    expect(donut.querySelectorAll(".recharts-pie-sector")).toHaveLength(1);
    expect(donut.querySelector(".recharts-pie-sector path")).toHaveAttribute("fill", "var(--muted)");
    expect(donut.querySelector(".recharts-pie-sector path")).not.toHaveAttribute("fill", "var(--a-cc)");
    expect(screen.queryByTestId("summary-legend")).not.toBeInTheDocument();
  });

  it("a long agent list item truncates inside its column", async () => {
    await show(overview({ byAgent: all(), today: total }));
    expect(legendItems()[0].querySelector("span")).toHaveClass("truncate");
    expect(screen.getByTestId("summary-legend")).toHaveClass("min-w-0");
  });
});

describe("Ringkasan share", () => {
  it("the Share button sits with the donut block and opens the panel in place of the tab", async () => {
    commands.share_preview = () => "data:image/png;base64,AAAA";
    const user = userEvent.setup();
    await show(overview({ byAgent: [12.8, 3.1, 0, 0, 0], today: { costUsd: 15.9 } }));
    const opener = screen.getByRole("button", { name: "Bagikan Pemakaian" });
    expect(opener.closest("section")).toContainElement(screen.getByRole("img", { name: /\$15,90/ }));
    expect(screen.getByTestId("summary-stats")).not.toContainElement(opener);
    await user.click(opener);
    const panel = await screen.findByRole("region", { name: "Bagikan Pemakaian" });
    expect(panel).toHaveFocus();
    expect(screen.queryByRole("heading", { name: "Hari Ini per Jam" })).not.toBeInTheDocument();
    expect(screen.getByTestId("ringkasan-body")).toHaveAttribute("hidden");
  });

  it("Escape closes the panel and focus returns to the Share button", async () => {
    commands.share_preview = () => "data:image/png;base64,AAAA";
    const user = userEvent.setup();
    await show(overview());
    const opener = screen.getByRole("button", { name: "Bagikan Pemakaian" });
    await user.click(opener);
    await screen.findByRole("region", { name: "Bagikan Pemakaian" });
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("region", { name: "Bagikan Pemakaian" })).not.toBeInTheDocument();
    expect(screen.getByTestId("ringkasan-body")).not.toHaveAttribute("hidden");
    expect(opener).toHaveFocus();
  });
});

describe("Ringkasan open and close", () => {
  const twoAgents = () => overview({ quotas: [ccQuota(), cxQuota()], byAgent: [12.8, 3.1, 0, 0, 0] });

  it("by default only the first quota section is open", async () => {
    await show(twoAgents());
    expect(toggle("cc")).toHaveAttribute("aria-expanded", "true");
    expect(toggle("cx")).toHaveAttribute("aria-expanded", "false");
    expect(toggle("cx")).toHaveAttribute("aria-controls", within(section("cx")).getAllByTestId("quota-row")[0].parentElement!.id);
  });

  it("toggles on click and from the keyboard", async () => {
    const user = userEvent.setup();
    await show(twoAgents());
    await user.click(toggle("cx"));
    expect(toggle("cx")).toHaveAttribute("aria-expanded", "true");
    toggle("cx").focus();
    await user.keyboard("{Enter}");
    expect(toggle("cx")).toHaveAttribute("aria-expanded", "false");
    await user.keyboard(" ");
    expect(toggle("cx")).toHaveAttribute("aria-expanded", "true");
  });

  it("the choice is saved and survives a remount", async () => {
    const user = userEvent.setup();
    const first = await show(twoAgents());
    await user.click(toggle("cx"));
    await user.click(toggle("cc"));
    first.unmount();
    await show(twoAgents());
    expect(toggle("cx")).toHaveAttribute("aria-expanded", "true");
    expect(toggle("cc")).toHaveAttribute("aria-expanded", "false");
  });

  it("storage that throws does not break the tab", async () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    const user = userEvent.setup();
    await show(twoAgents());
    expect(toggle("cc")).toHaveAttribute("aria-expanded", "true");
    await user.click(toggle("cx"));
    expect(toggle("cx")).toHaveAttribute("aria-expanded", "true");
  });

  it("garbage in storage is ignored", async () => {
    localStorage.setItem(STORAGE_KEY, "{nope");
    await show(twoAgents());
    expect(toggle("cc")).toHaveAttribute("aria-expanded", "true");
  });

  it("near its limit: open even if saved closed, and the user can still close it", async () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ cx: false }));
    const o = overview({ quotas: [ccQuota(), cxQuota(undefined, { nearLimit: true })], byAgent: [12.8, 3.1, 0, 0, 0] });
    await show(o);
    expect(toggle("cx")).toHaveAttribute("aria-expanded", "true");
    await userEvent.setup().click(toggle("cx"));
    expect(toggle("cx")).toHaveAttribute("aria-expanded", "false");
  });

  it("a saved choice beats the default", async () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ cc: false, cx: true }));
    await show(twoAgents());
    expect(toggle("cc")).toHaveAttribute("aria-expanded", "false");
    expect(toggle("cx")).toHaveAttribute("aria-expanded", "true");
  });
});

describe("Ringkasan today and RTK", () => {
  it("hourly section: a title only, no total, subtitle or chart legend", async () => {
    await show(overview());
    const heading = screen.getByRole("heading", { name: "Hari Ini per Jam" });
    expect(heading.nextElementSibling).toContainElement(screen.getByRole("group", { name: /per jam/ }));
    expect(heading.parentElement).not.toHaveTextContent(/token|Rp/);
    expect(screen.queryByTestId("stacked-legend")).not.toBeInTheDocument();
    expect(screen.getByRole("group", { name: /per jam/ })).toHaveStyle({ height: "120px" });
  });

  it("empty today: message instead of the chart", async () => {
    const o = overview({ quotas: [], byAgent: [0, 0, 0, 0, 0] });
    o.today = { ...o.today, costUsd: 0, tokens: 0, unpricedTokens: 0, hours: o.today.hours.map((h) => ({ ...h, byAgent: [0, 0, 0, 0, 0] })) };
    await show(o);
    expect(screen.getByText("Belum ada sesi hari ini")).toBeInTheDocument();
    expect(screen.queryByTestId("stacked-legend")).not.toBeInTheDocument();
  });

  it("today with only unpriced tokens: a dash and 'Harga Belum Ada', never $0,00 or 'no sessions'", async () => {
    const o = overview({ quotas: [], byAgent: [0, 0, 0, 0, 0] });
    o.today = {
      costUsd: 0,
      unpricedTokens: 5000,
      tokens: 5000,
      byAgent: [0, 0, 0, 0, 0],
      unpricedByAgent: [0, 0, 5000, 0, 0],
      hours: o.today.hours.map((h) => ({ ...h, byAgent: [0, 0, 0, 0, 0] })),
    };
    await show(o);
    expect(screen.getAllByText("Harga Belum Ada")).toHaveLength(1); // the OpenCode list item
    expect(screen.queryByText(/\$0,00/)).not.toBeInTheDocument();
    expect(screen.queryByText("Belum ada sesi hari ini")).not.toBeInTheDocument();
    expect(screen.getByText("Sesi hari ini belum punya harga")).toBeInTheDocument();
  });

  it("rtk null: no row; rtk present: tokens and approx cost", async () => {
    const a = overview({ rtk: null });
    await show(a);
    expect(screen.queryByText("Dihemat RTK")).not.toBeInTheDocument();
  });

  it("rtk present", async () => {
    await show(overview({ rtk: { savedTokens: 2_100_000, approxUsd: 6.3, model: "claude-sonnet-4-5" } }));
    expect(screen.getByText("Dihemat RTK")).toBeInTheDocument();
    expect(screen.getByText(/2,1 jt token/)).toBeInTheDocument();
    expect(screen.getByText(/≈ \$6,30/)).toBeInTheDocument();
  });
});

const orData = (over: Partial<OpenRouterData> = {}): OpenRouterData => ({
  free: false,
  balanceUsd: 62.75,
  todayUsd: 1.25,
  weekUsd: 6.5,
  monthUsd: 21.5,
  limit: null,
  ...over,
});
const orView = (over: Partial<OpenRouterData> = {}): OpenRouterView => ({ data: orData(over), unavailableReason: null });
const ag = (today: number, last7Days: number, over: Partial<NonNullable<Overview["antigravity"]>> = {}) => ({
  today,
  last7Days,
  lastActive: "2026-10-30T09:00:00Z",
  totalSessions: 121,
  totalSteps: 23021,
  ...over,
});
const orSection = () => screen.getByTestId("openrouter-section");
const orLines = () => within(orSection()).queryAllByTestId("openrouter-row").map((r) => r.textContent ?? "");

describe("Ringkasan OpenRouter", () => {
  it("sits below Cursor, not as an agent section, folded to the balance, with Per Pakai and the note", async () => {
    await show(overview({ quotas: [ccQuota(), crQuota()], byAgent: [12.8, 0, 0, 0, 1.72], openrouter: orView() }));
    expect(sections().map((s) => s.dataset.agent)).toEqual(["cc", "cr"]);
    expect(section("cr").compareDocumentPosition(orSection()) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(within(orSection()).getByText("OpenRouter")).toBeInTheDocument();
    expect(within(orSection()).getByText("Per Pakai")).toBeInTheDocument();
    expect(orLines()).toEqual(["Saldo$62,75"]);
    expect(within(orSection()).getByText("Tidak Ditambahkan Ke Total")).toBeVisible();
    expect(within(orSection()).getByRole("button", { expanded: false })).toBeInTheDocument();
  });

  it("without a readable balance the folded row is this month", async () => {
    await show(overview({ openrouter: orView({ balanceUsd: null }) }));
    expect(orLines()).toEqual(["Bulan Ini$21,50"]);
    await userEvent.setup().click(within(orSection()).getByRole("button", { expanded: false }));
    expect(orLines()).toEqual(["Hari Ini$1,25", "Minggu Ini$6,50", "Bulan Ini$21,50"]);
  });

  it("open: balance, today, week and month, in dollars", async () => {
    await show(overview({ openrouter: orView() }));
    await userEvent.setup().click(within(orSection()).getByRole("button", { expanded: false }));
    expect(orLines()).toEqual(["Saldo$62,75", "Hari Ini$1,25", "Minggu Ini$6,50", "Bulan Ini$21,50"]);
    expect(within(orSection()).queryByTestId("openrouter-limit")).not.toBeInTheDocument();
    expect(within(orSection()).getByText("Tidak Ditambahkan Ke Total")).toBeVisible();
  });

  it("a key with a limit shows what is left of it and when it resets, open only", async () => {
    const limit = { limitUsd: 50, remainingUsd: 28.5, reset: "monthly" as const };
    await show(overview({ openrouter: orView({ limit }) }));
    expect(within(orSection()).queryByTestId("openrouter-limit")).not.toBeInTheDocument();
    await userEvent.setup().click(within(orSection()).getByRole("button", { expanded: false }));
    const row = within(orSection()).getByTestId("openrouter-limit");
    expect(row).toHaveTextContent("Sisa Limit");
    expect(row).toHaveTextContent("$28,50 dari $50,00");
    expect(row).toHaveTextContent("Reset Bulanan");
    expect(row.querySelector<HTMLElement>("[data-slot=progress-indicator]")!.style.transform).toBe("translateX(-43%)"); // 57% left
    expect(row.querySelector(".text-destructive")).toBeNull();
  });

  it("a limit nearly used up turns red; a limit without a reset says none", async () => {
    await show(overview({ openrouter: orView({ limit: { limitUsd: 50, remainingUsd: 5, reset: null } }) }));
    await userEvent.setup().click(within(orSection()).getByRole("button", { expanded: false }));
    const row = within(orSection()).getByTestId("openrouter-limit");
    expect(row.querySelector(".text-destructive")).toHaveTextContent("$5,00 dari $50,00");
    expect(row).not.toHaveTextContent("Reset");
  });

  it("open: the usage is labelled as this key's, and an unreadable balance says so", async () => {
    await show(overview({ openrouter: orView({ balanceUsd: null }) }));
    expect(within(orSection()).queryByText("Pemakaian Key Ini")).not.toBeInTheDocument();
    expect(within(orSection()).queryByText("Saldo Tidak Tersedia Untuk Key Ini")).not.toBeInTheDocument();
    await userEvent.setup().click(within(orSection()).getByRole("button", { expanded: false }));
    expect(within(orSection()).getByText("Pemakaian Key Ini")).toBeInTheDocument();
    const note = within(orSection()).getByText("Saldo Tidak Tersedia Untuk Key Ini");
    expect(note).toBeInTheDocument();
    expect(within(orSection()).getByText("Pemakaian Key Ini").compareDocumentPosition(note) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("with a balance there is no 'unavailable' line", async () => {
    await show(overview({ openrouter: orView() }));
    await userEvent.setup().click(within(orSection()).getByRole("button", { expanded: false }));
    expect(within(orSection()).getByText("Pemakaian Key Ini")).toBeInTheDocument();
    expect(within(orSection()).queryByText("Saldo Tidak Tersedia Untuk Key Ini")).not.toBeInTheDocument();
  });

  it("free tier says Free", async () => {
    await show(overview({ openrouter: orView({ free: true }) }));
    expect(within(orSection()).getByText("Free")).toBeInTheDocument();
    expect(within(orSection()).queryByText("Per Pakai")).not.toBeInTheDocument();
  });

  it("the choice to open it is remembered apart from the agents", async () => {
    const user = userEvent.setup();
    const { unmount } = await show(overview({ openrouter: orView() }));
    await user.click(within(orSection()).getByRole("button", { expanded: false }));
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY)!)).toEqual({ or: true });
    unmount();
    await show(overview({ openrouter: orView() }));
    expect(within(orSection()).getByRole("button", { expanded: true })).toBeInTheDocument();
  });

  it("the note explains why the donut and the section differ", async () => {
    await show(overview({ openrouter: orView() }));
    const user = userEvent.setup();
    await user.hover(within(orSection()).getByRole("button", { name: "Kenapa Tidak Ditambahkan Ke Total" }));
    expect((await screen.findAllByText(/OpenCode atau Hermes yang sudah dihitung/))[0]).toBeInTheDocument();
  });

  it("unavailable: the reason and a button to Settings, no numbers and no fold", async () => {
    let opened = 0;
    commands.open_settings = () => void opened++;
    const reason = "Key OpenRouter Ditolak, Tempel Ulang Di Pengaturan";
    await show(overview({ openrouter: { data: null, unavailableReason: reason } }));
    const row = within(orSection()).getByTestId("openrouter-reason-row");
    expect(within(row).getByText(reason)).toBeInTheDocument();
    expect(orLines()).toEqual([]);
    expect(within(orSection()).queryByText("Tidak Ditambahkan Ke Total")).not.toBeInTheDocument();
    expect(within(orSection()).queryByRole("button", { expanded: false })).not.toBeInTheDocument();
    await userEvent.setup().click(within(row).getByRole("button", { name: "Buka Pengaturan" }));
    expect(opened).toBe(1);
  });

  it("not connected: no section", async () => {
    await show(overview({ openrouter: null }));
    expect(screen.queryByTestId("openrouter-section")).not.toBeInTheDocument();
  });

  it("changes nothing in the donut, the list, the totals or the hourly chart", async () => {
    const base = { quotas: [ccQuota(), crQuota()], byAgent: [12.8, 0, 0, 0, 1.72] as Costs, today: { costUsd: 14.52, tokens: 900_000 }, antigravity: null };
    const read = () => ({
      donut: screen.getByRole("img", { name: /\$14,52/ }).textContent?.replace(/\s*\[data-chart[\s\S]*$/, ""),
      legend: legendItems().map((li) => li.textContent),
      stats: screen.getByTestId("summary-stats").textContent,
      segments: screen.getByRole("img", { name: /\$14,52/ }).querySelectorAll(".recharts-pie-sector").length,
    });
    const first = await show(overview({ ...base, openrouter: null }));
    const without = read();
    first.unmount();
    await show(overview({ ...base, openrouter: orView({ todayUsd: 99, monthUsd: 999 }) }));
    expect(read()).toEqual(without);
    expect(without.legend).toEqual(["Claude Code$12,80", "Cursor$1,72"]);
  });
});

describe("Ringkasan Antigravity", () => {
  const row = () => screen.getByTestId("antigravity-row");

  it("one fixed row: name, Aktivitas and the two counts, nothing to fold", async () => {
    await show(overview({ antigravity: ag(3, 12) }));
    expect(within(row()).getByText("Antigravity")).toBeInTheDocument();
    expect(within(row()).getByText("Aktivitas")).toBeInTheDocument();
    expect(row()).toHaveTextContent("3 Sesi Hari Ini · 12 Dalam 7 Hari");
    expect(row().querySelector("svg use")).toHaveAttribute("href", "#lg-antigravity");
    expect(within(row()).queryByText("AG")).not.toBeInTheDocument();
    expect(within(row()).queryByRole("button")).not.toBeInTheDocument();
    expect(sections().map((s) => s.dataset.agent)).not.toContain("ag");
  });

  it("nothing in 7 days: the totals and the date of the last session, in the same row", async () => {
    await show(overview({ antigravity: ag(0, 0, { lastActive: "2026-07-09T09:29:13Z" }) }));
    expect(row()).toHaveTextContent("121 Sesi · 23.021 Langkah · Terakhir 9 Jul");
    expect(row()).not.toHaveTextContent("Dalam 7 Hari");
    expect(within(row()).getByText("Aktivitas")).toBeInTheDocument();
    expect(row().textContent).not.toMatch(/token|\$|biaya|kuota/i);
  });

  it("old sessions without steps or a date still say how many there are", async () => {
    await show(overview({ antigravity: ag(0, 0, { totalSessions: 1, totalSteps: 0, lastActive: null }) }));
    expect(row()).toHaveTextContent(/Aktivitas1 Sesi$/);
  });

  it("the second line may wrap but never overflows the row", async () => {
    await show(overview({ antigravity: ag(0, 0) }));
    expect(row().querySelector("p")!.className).toContain("break-words");
  });

  it("no activity data: no row, and it brings no token, cost or quota", async () => {
    await show(overview({ antigravity: null }));
    expect(screen.queryByTestId("antigravity-row")).not.toBeInTheDocument();
  });

  it("is not part of the donut or the list", async () => {
    await show(overview({ antigravity: ag(3, 12), today: { costUsd: 12.8 } }));
    expect(legendItems().map((li) => li.textContent)).toEqual(["Claude Code$12,80"]);
    expect(within(row()).queryByText(/\$/)).not.toBeInTheDocument();
  });

  it("comes after OpenRouter", async () => {
    await show(overview({ openrouter: orView(), antigravity: ag(1, 2) }));
    expect(orSection().compareDocumentPosition(row()) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});
