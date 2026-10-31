import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import historyFixture from "../test/fixtures/history-view.json";
import settingsFixture from "../test/fixtures/settings.json";
import { stubChartLayout } from "../test/layout";
import { commands, invokeMock, listenMock, resetTauri } from "../test/tauri";
import type { DayCost, HistoryView, PlanValue } from "../lib/api";
import { dayLabel } from "../lib/format";
import { resetUsdToIdrCache } from "../hooks/useUsdToIdr";

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: [string, unknown?]) => invokeMock(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: Parameters<typeof listenMock>) => listenMock(...a) }));

import { Riwayat, axisLabelIndices } from "./Riwayat";

beforeAll(() => stubChartLayout());
afterAll(() => vi.unstubAllGlobals());

const days = (n: number): DayCost[] =>
  Array.from({ length: n }, (_, i) => ({
    date: `2026-10-${String(30 - n + 1 + i).padStart(2, "0")}`,
    byAgent: [10 + (i % 5), 3, 0, 0, 0],
    unpricedTokens: 0,
  }));

const plan = (over: Partial<PlanValue>): PlanValue => ({
  ...(historyFixture.plans[0] as unknown as PlanValue),
  cycle: { start: "2026-11-01", endExclusive: "2026-12-01", dayOfCycle: 12, length: 30 },
  ...over,
});

const max5 = plan({ planId: "claude-max", name: "Claude Max 5×", agents: ["cc"], priceUsd: 100, apiEquivalentUsd: 770, multiple: 7.7, projectedUsd: 1500, projectedMultiple: 15 });
const plus = plan({ planId: "chatgpt-plus", name: "ChatGPT Plus (Codex)", agents: ["cx"], priceUsd: 20, apiEquivalentUsd: 56, multiple: 2.8, projectedUsd: 140, projectedMultiple: 7 });

const view = (over: Partial<HistoryView> = {}): HistoryView => ({
  days: days(30),
  totalUsd: 840.91,
  avgPerDay: 28.03,
  unpricedTokens: 0,
  plans: [max5, plus],
  ...over,
});

beforeEach(() => {
  resetTauri();
  resetUsdToIdrCache();
  commands.get_settings = () => ({ ...settingsFixture, usdToIdr: 16300 });
  commands.get_history = () => view();
});

const historyCalls = () => invokeMock.mock.calls.filter(([c]) => c === "get_history");

describe("axisLabelIndices", () => {
  it("labels every n-th day and always the last, dropping a label closer than half an interval to it", () => {
    expect(axisLabelIndices(30, 7)).toEqual([0, 7, 14, 21, 29]);
    expect(axisLabelIndices(14, 7)).toEqual([0, 7, 13]);
    expect(axisLabelIndices(9, 7)).toEqual([0, 8]);
    expect(axisLabelIndices(7, 7)).toEqual([0, 6]);
    expect(axisLabelIndices(1, 7)).toEqual([0]);
    expect(axisLabelIndices(0, 7)).toEqual([]);
  });
});

describe("Riwayat: Biaya Harian", () => {
  it("shows the big total and 'Rp… · Rata-rata $…/hari'", async () => {
    render(<Riwayat />);
    expect(await screen.findByText("Biaya Harian")).toBeInTheDocument();
    expect(screen.getByText("$840,91")).toBeInTheDocument();
    expect(screen.getByText("Rp13,71 jt · Rata-rata $28,03/hari")).toBeInTheDocument();
  });

  it("offers 7/14/30 days, defaults to 30 and refetches with the chosen length", async () => {
    const user = userEvent.setup();
    render(<Riwayat />);
    await screen.findByText("Biaya Harian");
    expect(historyCalls().slice(-1)[0]?.[1]).toEqual({ days: 30 });
    expect(screen.getByRole("combobox", { name: "Periode" })).toHaveTextContent("30 Hari Terakhir");
    await user.click(screen.getByRole("combobox", { name: "Periode" }));
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["7 Hari Terakhir", "14 Hari Terakhir", "30 Hari Terakhir"]);
    await user.click(screen.getByRole("option", { name: "7 Hari Terakhir" }));
    await vi.waitFor(() => expect(historyCalls().slice(-1)[0]?.[1]).toEqual({ days: 7 }));
  });

  it("labels the x axis with day of month every 7 days, last always shown without collisions, today bold", async () => {
    render(<Riwayat />);
    const chart = within(await screen.findByRole("group", { name: /Biaya harian/i }));
    for (const l of ["1", "8", "15", "22", "30"]) expect(chart.getByText(l)).toBeInTheDocument();
    expect(chart.queryByText("29")).not.toBeInTheDocument();
    expect(chart.getByText("30")).toHaveAttribute("font-weight", "600");
    expect(chart.getByText("22")).toHaveAttribute("font-weight", "400");
  });

  it("shows y ticks in whole dollars", async () => {
    commands.get_history = () => view({ days: days(30).map((d) => ({ ...d, byAgent: [40, 15, 0, 0, 0] })) });
    render(<Riwayat />);
    const chart = within(await screen.findByRole("group", { name: /Biaya harian/i }));
    for (const t of ["$0", "$20", "$40", "$60"]) expect(chart.getByText(t)).toBeInTheDocument();
  });

  it("tooltip title is the Indonesian day label, with per-agent amounts and a total", async () => {
    const user = userEvent.setup();
    render(<Riwayat />);
    const group = await screen.findByRole("group", { name: /Biaya harian/i });
    const surface = group.querySelector<SVGElement>(".recharts-surface")!;
    act(() => surface.focus());
    for (let i = 0; i < 30; i++) await user.keyboard("{ArrowRight}");
    expect(await within(group).findByText("Jumlah")).toBeInTheDocument();
    expect(within(group).getByText(dayLabel("2026-10-30"))).toBeInTheDocument();
  });

  it("a day with tokens but no known price says so in the tooltip", async () => {
    const list = days(7).map((d, i) => (i === 6 ? { ...d, byAgent: [0, 0, 0, 0, 0] as DayCost["byAgent"], unpricedTokens: 800 } : d));
    commands.get_history = () => view({ days: list });
    const user = userEvent.setup();
    render(<Riwayat />);
    const group = await screen.findByRole("group", { name: /Biaya harian/i });
    act(() => group.querySelector<SVGElement>(".recharts-surface")!.focus());
    for (let i = 0; i < 7; i++) await user.keyboard("{ArrowRight}");
    expect(await within(group).findByText("Harga Belum Ada")).toBeInTheDocument();
    expect(within(group).queryByText("Tidak ada sesi")).not.toBeInTheDocument();
  });

  it("period with only unpriced tokens: a dash and 'Harga Belum Ada', never $0,00", async () => {
    const list = days(7).map((d) => ({ ...d, byAgent: [0, 0, 0, 0, 0] as DayCost["byAgent"], unpricedTokens: 100 }));
    commands.get_history = () => view({ days: list, totalUsd: 0, avgPerDay: 0, unpricedTokens: 700, plans: [] });
    render(<Riwayat />);
    await screen.findByText("Biaya Harian");
    expect(screen.getByText("Harga Belum Ada")).toBeInTheDocument();
    expect(screen.getByText("–")).toBeInTheDocument();
    expect(screen.queryByText(/\$0,00/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Rata-rata/)).not.toBeInTheDocument();
  });

  it("has a legend with agents that have cost", async () => {
    render(<Riwayat />);
    await screen.findByText("Biaya Harian");
    const legend = within(screen.getByTestId("stacked-legend"));
    expect(legend.getByText("Claude Code")).toBeInTheDocument();
    expect(legend.getByText("Codex")).toBeInTheDocument();
    expect(legend.queryByText("OpenCode")).not.toBeInTheDocument();
  });
});

describe("Riwayat: Nilai Langganan", () => {
  it("shows the section sub and the total payback badge (sum api / sum price)", async () => {
    render(<Riwayat />);
    expect(await screen.findByText("Nilai Langganan")).toBeInTheDocument();
    expect(screen.getByText("Bayar $120/bln · Setara $826 API")).toBeInTheDocument();
    expect(screen.getByText("Balik Modal 6,9×")).toBeInTheDocument();
  });

  it("hides the payback badge when the plans cost nothing", async () => {
    const free = plan({ planId: "free", name: "Gratis", agents: ["cc"], priceUsd: 0, apiEquivalentUsd: 56, multiple: 0, projectedUsd: 140, projectedMultiple: 0 });
    commands.get_history = () => view({ plans: [free] });
    render(<Riwayat />);
    expect(await screen.findByText("Nilai Langganan")).toBeInTheDocument();
    expect(screen.queryByText(/Balik Modal/)).not.toBeInTheDocument();
    expect(screen.queryByText(/NaN|Infinity/)).not.toBeInTheDocument();
  });

  it("per plan: name, bar with price tick, 'Bayar → setara' and ratio", async () => {
    render(<Riwayat />);
    await screen.findByText("Nilai Langganan");
    const row = screen.getByText("Claude Max 5×").closest("li")!;
    expect(within(row).getByText("Bayar $100 → setara $770")).toBeInTheDocument();
    expect(within(row).getByText("7,7×")).toBeInTheDocument();
    const bar = within(row).getByRole("img", { name: "Claude Max 5×: bayar $100, setara $770 API, 7,7 kali" });
    expect(bar).toBeInTheDocument();
    const fill = within(bar).getByTestId("value-fill");
    expect(fill.getAttribute("style")).toContain("var(--a-cc)");
    expect(parseFloat(fill.style.width)).toBeCloseTo((770 / (770 * 1.05)) * 100, 1);
    const tick = within(bar).getByTestId("price-tick");
    expect(tick.className).toMatch(/w-0\.5|w-\[2px\]/);
    expect(parseFloat(tick.style.left)).toBeCloseTo((100 / (770 * 1.05)) * 100, 1);
    // second plan is scaled against the same maximum
    const row2 = screen.getByText("ChatGPT Plus (Codex)").closest("li")!;
    expect(within(row2).getByTestId("value-fill").getAttribute("style")).toContain("var(--a-cx)");
    expect(parseFloat(within(row2).getByTestId("value-fill").style.width)).toBeCloseTo((56 / (770 * 1.05)) * 100, 1);
  });

  it("shows the cycle label and the projection", async () => {
    render(<Riwayat />);
    await screen.findByText("Nilai Langganan");
    const row = screen.getByText("Claude Max 5×").closest("li")!;
    expect(within(row).getByText("Siklus 1–30 Nov · Hari ke-12")).toBeInTheDocument();
    expect(within(row).getByText("Proyeksi akhir siklus ≈ $1.500 (15×)")).toBeInTheDocument();
  });

  it("labels a cycle that spans two months with both months", async () => {
    commands.get_history = () =>
      view({ plans: [plan({ cycle: { start: "2026-10-15", endExclusive: "2026-11-15", dayOfCycle: 16, length: 31 } })] });
    render(<Riwayat />);
    expect(await screen.findByText("Siklus 15 Okt–14 Nov · Hari ke-16")).toBeInTheDocument();
  });

  it("is hidden when there are no plans", async () => {
    commands.get_history = () => view({ plans: [] });
    render(<Riwayat />);
    await screen.findByText("Biaya Harian");
    expect(screen.queryByText("Nilai Langganan")).not.toBeInTheDocument();
    expect(screen.queryByText(/Balik Modal/)).not.toBeInTheDocument();
  });

  it("does not change when the period changes", async () => {
    const user = userEvent.setup();
    render(<Riwayat />);
    await screen.findByText("Nilai Langganan");
    const section = () => screen.getByText("Nilai Langganan").closest("section")!;
    const before = section().innerHTML;
    commands.get_history = (a) => view({ days: days((a as { days: number }).days), totalUsd: 100, avgPerDay: 14.28 });
    await user.click(screen.getByRole("combobox", { name: "Periode" }));
    await user.click(screen.getByRole("option", { name: "7 Hari Terakhir" }));
    expect(await screen.findByText("$100,00")).toBeInTheDocument();
    expect(section().innerHTML).toBe(before);
  });
});
