import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import pageFixture from "../test/fixtures/session-page.json";
import settingsFixture from "../test/fixtures/settings.json";
import { commands, emit, invokeMock, listenMock, resetTauri } from "../test/tauri";
import type { DayGroup, SessionPage, SessionRow } from "../lib/api";
import { clock } from "../lib/format";
import { resetUsdToIdrCache } from "../hooks/useUsdToIdr";

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: [string, unknown?]) => invokeMock(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: Parameters<typeof listenMock>) => listenMock(...a) }));

import { Mutasi } from "./Mutasi";

const page = (over: Partial<SessionPage> = {}): SessionPage => ({ ...(pageFixture as unknown as SessionPage), ...over });
const row = (over: Partial<SessionRow> = {}): SessionRow => ({
  ...(pageFixture.days[0].sessions[0] as unknown as SessionRow),
  ...over,
});
const day = (date: string, label: string, sessions: SessionRow[], costUsd = 1, unpricedTokens = 0): DayGroup => ({ date, label, costUsd, unpricedTokens, sessions });

const calls = () => invokeMock.mock.calls.filter(([c]) => c === "get_sessions").map(([, a]) => (a as { filter: unknown }).filter);

beforeEach(() => {
  resetTauri();
  resetUsdToIdrCache();
  commands.get_settings = () => ({ ...settingsFixture, usdToIdr: 16300 });
  commands.get_sessions = () => page();
});

describe("Mutasi", () => {
  it("shows the day header with the backend label and total in USD and IDR", async () => {
    render(<Mutasi />);
    expect(await screen.findByText("Hari Ini · Jum, 30 Okt")).toBeInTheDocument();
    expect(screen.getByText("$15,25 · Rp248.575")).toBeInTheDocument();
  });

  it("renders a session row with project, 'Jam · Model · Durasi', cost and tokens", async () => {
    render(<Mutasi />);
    const items = await screen.findAllByRole("listitem");
    const first = items[0];
    expect(within(first).getByText("buku-token")).toBeInTheDocument();
    expect(within(first).getByText(`${clock("2026-10-30T05:40:00Z")} · Opus 5.5 · 1j 12m`)).toBeInTheDocument();
    expect(within(first).getByText("$15,25")).toBeInTheDocument();
    expect(within(first).getByText("612 rb token")).toBeInTheDocument();
  });

  it("shows an unknown cost as a dash with a 'Harga Belum Ada' badge, never $0,00", async () => {
    render(<Mutasi />);
    const items = await screen.findAllByRole("listitem");
    const second = items[1];
    expect(within(second).getByText("–")).toBeInTheDocument();
    expect(within(second).getByText("Harga Belum Ada")).toBeInTheDocument();
    expect(screen.queryByText("$0,00")).not.toBeInTheDocument();
  });

  it("offers 'Muat 3 Hari Sebelumnya' only when hasMore", async () => {
    const { unmount } = render(<Mutasi />);
    expect(await screen.findByRole("button", { name: "Muat 3 Hari Sebelumnya" })).toBeInTheDocument();
    unmount();
    commands.get_sessions = () => page({ hasMore: false });
    render(<Mutasi />);
    await screen.findByText("Hari Ini · Jum, 30 Okt");
    expect(screen.queryByRole("button", { name: "Muat 3 Hari Sebelumnya" })).not.toBeInTheDocument();
  });

  it("loads older days with before = oldest loaded date and appends them", async () => {
    const older = day("2026-10-27", "Senin, 27 Okt", [row({ sessionId: "o1", project: { path: "/x", name: "proyek-lama", displayPath: "x" } })]);
    commands.get_sessions = (a) => ((a as { filter: { before?: string } }).filter.before ? page({ days: [older], hasMore: false }) : page());
    const user = userEvent.setup();
    render(<Mutasi />);
    await user.click(await screen.findByRole("button", { name: "Muat 3 Hari Sebelumnya" }));
    expect(await screen.findByText("proyek-lama")).toBeInTheDocument();
    expect(screen.getByText("Hari Ini · Jum, 30 Okt")).toBeInTheDocument();
    expect(calls().slice(-1)[0]).toMatchObject({ before: "2026-10-30", days: 3 });
    expect(screen.queryByRole("button", { name: "Muat 3 Hari Sebelumnya" })).not.toBeInTheDocument();
  });

  it("opens the filter below its trigger, with agent logos in the options", async () => {
    commands.get_settings = () => ({ ...settingsFixture, enabledAgents: {} }); // every agent on
    const user = userEvent.setup();
    render(<Mutasi />);
    await user.click(await screen.findByRole("combobox", { name: "Filter agent" }));
    const claude = await screen.findByRole("option", { name: "Claude Code" });
    // item-aligned positioning covers the trigger and its neighbour; popper drops below them.
    expect(document.querySelector("[data-slot=select-content]")).toHaveAttribute("data-align-trigger", "false");
    expect(claude.querySelector("use")).toHaveAttribute("href", "#lg-claude");
    expect(within(screen.getByRole("option", { name: "OpenCode" })).getByText("OC")).toBeInTheDocument();
  });

  it("lists only the agents that are switched on", async () => {
    const user = userEvent.setup();
    render(<Mutasi />);
    await user.click(await screen.findByRole("combobox", { name: "Filter agent" }));
    const names = (await screen.findAllByRole("option")).map((o) => o.textContent);
    expect(names).toEqual(["Semua Agent", "Claude Code", "Codex", "Hermes", "Cursor"]);
  });

  it("goes back to Semua Agent when the chosen agent gets switched off", async () => {
    const user = userEvent.setup();
    render(<Mutasi />);
    await user.click(await screen.findByRole("combobox", { name: "Filter agent" }));
    await user.click(await screen.findByRole("option", { name: "Codex" }));
    await vi.waitFor(() => expect(calls().slice(-1)[0]).toMatchObject({ agent: "cx" }));
    commands.get_settings = () => ({ ...settingsFixture, enabledAgents: { ...settingsFixture.enabledAgents, cx: false } });
    emit("data-updated");
    await vi.waitFor(() => expect(calls().slice(-1)[0]).toMatchObject({ agent: null }));
    expect(screen.getByRole("combobox", { name: "Filter agent" })).toHaveTextContent("Semua Agent");
  });

  it("gives both filters the same flexible width so a long project name truncates", async () => {
    render(<Mutasi />);
    const [agent, project] = [await screen.findByRole("combobox", { name: "Filter agent" }), screen.getByRole("combobox", { name: "Filter project" })];
    for (const trigger of [agent, project]) expect(trigger).toHaveClass("min-w-0", "flex-1");
  });

  it("filters by agent and by project, mapping to get_sessions and resetting the list", async () => {
    const older = day("2026-10-27", "Senin, 27 Okt", [row({ sessionId: "o1", project: { path: "/x", name: "proyek-lama", displayPath: "x" } })]);
    commands.get_sessions = (a) => ((a as { filter: { before?: string } }).filter.before ? page({ days: [older], hasMore: false }) : page());
    const user = userEvent.setup();
    render(<Mutasi />);
    await user.click(await screen.findByRole("button", { name: "Muat 3 Hari Sebelumnya" }));
    await screen.findByText("proyek-lama");

    await user.click(screen.getByRole("combobox", { name: "Filter agent" }));
    await user.click(await screen.findByRole("option", { name: "Codex" }));
    await vi.waitFor(() => expect(calls().slice(-1)[0]).toMatchObject({ agent: "cx", projectPath: null, days: 3 }));
    expect(calls().slice(-1)[0]).not.toHaveProperty("before", "2026-10-30");
    await vi.waitFor(() => expect(screen.queryByText("proyek-lama")).not.toBeInTheDocument());

    await user.click(screen.getByRole("combobox", { name: "Filter project" }));
    await user.click(await screen.findByRole("option", { name: "buku-token" }));
    await vi.waitFor(() => expect(calls().slice(-1)[0]).toMatchObject({ agent: "cx", projectPath: "/Users/dev/Documents/buku-token" }));
  });

  it("offers Semua Agent / Semua Project and 'Tanpa Project' maps to an empty path", async () => {
    commands.get_sessions = () =>
      page({ projects: [...page().projects, { path: "", name: "Tanpa Project", displayPath: "" }] });
    const user = userEvent.setup();
    render(<Mutasi />);
    await screen.findByText("Hari Ini · Jum, 30 Okt");
    expect(screen.getByRole("combobox", { name: "Filter agent" })).toHaveTextContent("Semua Agent");
    expect(screen.getByRole("combobox", { name: "Filter project" })).toHaveTextContent("Semua Project");
    await user.click(screen.getByRole("combobox", { name: "Filter project" }));
    await user.click(await screen.findByRole("option", { name: "Tanpa Project" }));
    await vi.waitFor(() => expect(calls().slice(-1)[0]).toMatchObject({ projectPath: "" }));
  });

  it("uses the shared rate hook: settings are fetched once", async () => {
    render(<Mutasi />);
    await screen.findByText("Hari Ini · Jum, 30 Okt");
    expect(invokeMock.mock.calls.filter(([c]) => c === "get_settings")).toHaveLength(1);
  });

  it("day with tokens but no known price: a dash and 'Harga Belum Ada', never $0,00", async () => {
    const unpriced = row({ sessionId: "u1", costUsd: null, unpricedTokens: 500 });
    commands.get_sessions = () => page({ days: [day("2026-10-30", "Hari Ini · Jum, 30 Okt", [unpriced], 0, 500)] });
    render(<Mutasi />);
    const heading = await screen.findByRole("heading", { name: /Hari Ini/ });
    expect(within(heading).getByText("–")).toBeInTheDocument();
    expect(within(heading).getByText("Harga Belum Ada")).toBeInTheDocument();
    expect(screen.queryByText(/\$0,00/)).not.toBeInTheDocument();
  });

  it("a truly free day (no unpriced tokens) still reads $0,00", async () => {
    commands.get_sessions = () => page({ days: [day("2026-10-30", "Hari Ini · Jum, 30 Okt", [row({ costUsd: 0 })], 0, 0)] });
    render(<Mutasi />);
    const heading = await screen.findByRole("heading", { name: /Hari Ini/ });
    expect(within(heading).getByText("$0,00 · Rp0")).toBeInTheDocument();
  });

  it("shows an empty state when there are no sessions", async () => {
    commands.get_sessions = () => page({ days: [], hasMore: false });
    render(<Mutasi />);
    expect(await screen.findByText("Belum ada sesi")).toBeInTheDocument();
  });
});

describe("Mutasi paging across refreshes", () => {
  const olderDay = (date: string, name: string) =>
    day(date, `Label ${date}`, [row({ sessionId: `o-${date}`, project: { path: `/${name}`, name, displayPath: name } })]);
  const firstPages = () => calls().filter((f) => !(f as { before?: string }).before);
  const LOAD = { name: "Muat 3 Hari Sebelumnya" };

  it("keeps loaded days when data-updated refetches, and refetches what is on screen", async () => {
    const older = olderDay("2026-10-27", "proyek-lama");
    commands.get_sessions = (a) => ((a as { filter: { before?: string } }).filter.before ? page({ days: [older], hasMore: false }) : page());
    const user = userEvent.setup();
    render(<Mutasi />);
    await user.click(await screen.findByRole("button", LOAD));
    await screen.findByText("proyek-lama");
    const before = firstPages().length;

    act(() => emit("data-updated"));
    await waitFor(() => expect(firstPages().length).toBe(before + 1));
    await waitFor(() => expect(screen.queryByTestId("data-view")).toHaveStyle({ opacity: "1" }));
    expect(screen.getByText("proyek-lama")).toBeInTheDocument();
    // 3 days plus the one loaded, so a new day at the top cannot open a gap in the list.
    expect(firstPages().slice(-1)[0]).toMatchObject({ days: 4 });
    expect(firstPages().slice(-1)[0]).not.toHaveProperty("before");
    expect(screen.queryByRole("button", LOAD)).not.toBeInTheDocument();
  });

  it("merges by date: a refreshed page wins over the same day loaded earlier", async () => {
    const stale = olderDay("2026-10-27", "versi-lama");
    const fresh = olderDay("2026-10-27", "versi-baru");
    let refreshed = false;
    commands.get_sessions = (a) => {
      if ((a as { filter: { before?: string } }).filter.before) return page({ days: [stale], hasMore: false });
      return refreshed ? page({ days: [...page().days, fresh], hasMore: false }) : page();
    };
    const user = userEvent.setup();
    render(<Mutasi />);
    await user.click(await screen.findByRole("button", LOAD));
    await screen.findByText("versi-lama");

    refreshed = true;
    act(() => emit("data-updated"));
    expect(await screen.findByText("versi-baru")).toBeInTheDocument();
    expect(screen.queryByText("versi-lama")).not.toBeInTheDocument();
    expect(screen.getAllByText("Label 2026-10-27")).toHaveLength(1);
  });

  it("a refresh that lands while a page is loading does not lose that page", async () => {
    const older = olderDay("2026-10-27", "proyek-lama");
    let release!: (p: SessionPage) => void;
    commands.get_sessions = (a) =>
      (a as { filter: { before?: string } }).filter.before ? new Promise<SessionPage>((r) => (release = r)) : page();
    const user = userEvent.setup();
    render(<Mutasi />);
    await user.click(await screen.findByRole("button", LOAD));
    const before = firstPages().length;

    act(() => emit("data-updated"));
    await waitFor(() => expect(firstPages().length).toBe(before + 1));
    await act(async () => release(page({ days: [older], hasMore: false })));
    expect(await screen.findByText("proyek-lama")).toBeInTheDocument();
    expect(screen.getByText("Hari Ini · Jum, 30 Okt")).toBeInTheDocument();
  });

  it("a page requested for the previous filter is dropped", async () => {
    const older = olderDay("2026-10-27", "proyek-lama");
    let release!: (p: SessionPage) => void;
    commands.get_sessions = (a) =>
      (a as { filter: { before?: string } }).filter.before ? new Promise<SessionPage>((r) => (release = r)) : page();
    const user = userEvent.setup();
    render(<Mutasi />);
    await user.click(await screen.findByRole("button", LOAD));

    await user.click(screen.getByRole("combobox", { name: "Filter agent" }));
    await user.click(await screen.findByRole("option", { name: "Codex" }));
    await vi.waitFor(() => expect(firstPages().slice(-1)[0]).toMatchObject({ agent: "cx", days: 3 }));
    await act(async () => release(page({ days: [older], hasMore: false })));
    expect(screen.queryByText("proyek-lama")).not.toBeInTheDocument();

    act(() => emit("data-updated"));
    await waitFor(() => expect(firstPages().slice(-1)[0]).toMatchObject({ agent: "cx", days: 3 }));
    expect(firstPages().every((f) => (f as { days: number }).days === 3)).toBe(true);
  });

  it("a filter change goes back to one page", async () => {
    const older = olderDay("2026-10-27", "proyek-lama");
    commands.get_sessions = (a) => ((a as { filter: { before?: string } }).filter.before ? page({ days: [older], hasMore: false }) : page());
    const user = userEvent.setup();
    render(<Mutasi />);
    await user.click(await screen.findByRole("button", LOAD));
    await screen.findByText("proyek-lama");

    await user.click(screen.getByRole("combobox", { name: "Filter agent" }));
    await user.click(await screen.findByRole("option", { name: "Codex" }));
    await vi.waitFor(() => expect(firstPages().slice(-1)[0]).toMatchObject({ agent: "cx", days: 3 }));
    await vi.waitFor(() => expect(screen.queryByText("proyek-lama")).not.toBeInTheDocument());
  });
});
