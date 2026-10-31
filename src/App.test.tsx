import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import settingsFixture from "./test/fixtures/settings.json";
import statusFixture from "./test/fixtures/scan-status.json";
import overviewFixture from "./test/fixtures/overview-quota.json";
import sessionFixture from "./test/fixtures/session-page.json";
import { commands, emit, invokeMock, listenMock, resetTauri } from "./test/tauri";

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: [string, unknown?]) => invokeMock(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: Parameters<typeof listenMock>) => listenMock(...a) }));

import App from "./App";
import { resetUsdToIdrCache } from "./hooks/useUsdToIdr";
import { setTheme } from "./lib/theme";

const NOW = new Date("2026-10-30T10:00:00Z");
const status = (over: object = {}) => ({
  ...statusFixture,
  running: false,
  lastScanAt: new Date(NOW.getTime() - 90_000).toISOString(),
  ...over,
});

let resizeCb: () => void = () => {};
beforeEach(() => {
  resetTauri();
  resizeCb = () => {};
  commands.get_settings = () => ({ ...settingsFixture, onboarded: true, lastTab: "ringkasan" });
  commands.scan_status = () => status();
  commands.save_settings = () => ({});
  commands.resize_popover = () => null;
  commands.refresh = () => null;
  commands.open_settings = () => null;
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(cb: () => void) {
        resizeCb = cb;
      }
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ height: 412.4 } as DOMRect);
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("popover shell", () => {
  it("renders_five_tabs_in_order", async () => {
    render(<App />);
    const tabs = await screen.findAllByRole("tab");
    expect(tabs.map((t) => t.textContent)).toEqual(["Ringkasan", "Mutasi", "Model", "Project", "Riwayat"]);
  });

  it("moves_initial_focus_to_the_popover_not_a_tab", async () => {
    render(<App />);
    const tabs = await screen.findAllByRole("tab");
    const root = screen.getByTestId("popover-root");
    expect(root).toHaveAttribute("tabindex", "-1");
    expect(document.activeElement).toBe(root);
    expect(tabs.some((t) => t === document.activeElement)).toBe(false);
    expect(screen.getByRole("tablist").contains(document.activeElement)).toBe(false);
  });

  it("keyboard_navigation_between_tabs_still_works", async () => {
    const user = userEvent.setup();
    render(<App />);
    const first = await screen.findByRole("tab", { name: "Ringkasan" });
    await user.tab();
    expect(first).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Mutasi" })).toHaveFocus();
  });

  it("tab_triggers_use_focus_visible_only", async () => {
    render(<App />);
    for (const t of await screen.findAllByRole("tab")) {
      expect(t.className).toMatch(/focus-visible:ring/);
      expect(t.className).not.toMatch(/(^|\s)focus:/);
    }
  });

  it("restores_last_tab_from_settings", async () => {
    commands.get_settings = () => ({ ...settingsFixture, onboarded: true, lastTab: "model" });
    render(<App />);
    const model = await screen.findByRole("tab", { name: "Model" });
    expect(model).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Ringkasan" })).toHaveAttribute("aria-selected", "false");
  });

  it("falls_back_to_ringkasan_for_unknown_last_tab", async () => {
    commands.get_settings = () => ({ ...settingsFixture, onboarded: true, lastTab: "bogus" });
    render(<App />);
    expect(await screen.findByRole("tab", { name: "Ringkasan" })).toHaveAttribute("aria-selected", "true");
  });

  it("saves_last_tab_on_change", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("tab", { name: "Mutasi" }));
    expect(invokeMock).toHaveBeenCalledWith("save_settings", { settings: { lastTab: "mutasi" } });
  });

  it("first_scan_shows_progress", async () => {
    commands.scan_status = () => status({ running: true, lastScanAt: null, done: 0, total: 958 });
    render(<App />);
    await screen.findByText(/Membaca log…/);
    act(() => emit("scan-progress", { done: 312, total: 958 }));
    expect(await screen.findByText("Membaca log… 312/958 file")).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toBeInTheDocument();
    expect(screen.queryByRole("tab")).not.toBeInTheDocument();

    commands.scan_status = () => status();
    act(() => emit("data-updated"));
    expect(await screen.findAllByRole("tab")).toHaveLength(5);
  });

  it("status_bar_shows_relative_update", async () => {
    vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"] });
    vi.setSystemTime(NOW);
    render(<App />);
    expect(await screen.findByText("Diperbarui 1 menit lalu")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(30_000 * 8);
    });
    expect(screen.getByText("Diperbarui 5 menit lalu")).toBeInTheDocument();
  });

  it("refresh_button_calls_refresh_and_settings_button_is_safe", async () => {
    commands.open_settings = () => {
      throw new Error("no window");
    };
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Perbarui" }));
    expect(invokeMock).toHaveBeenCalledWith("refresh");
    await user.click(screen.getByRole("button", { name: "Pengaturan" }));
    expect(invokeMock).toHaveBeenCalledWith("open_settings");
  });

  it("draws_the_transparent_window_surface_on_the_root", async () => {
    render(<App />);
    await screen.findAllByRole("tab");
    const shell = screen.getByTestId("popover-root");
    expect(shell).toHaveClass("bg-background", "rounded-[16px]", "border");
  });

  it("reports_height_to_resize_popover", async () => {
    render(<App />);
    await screen.findAllByRole("tab");
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("resize_popover", { height: 413 }));
    vi.mocked(HTMLElement.prototype.getBoundingClientRect).mockReturnValue({ height: 520 } as DOMRect);
    act(() => resizeCb());
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("resize_popover", { height: 520 }));
  });

  it("does_not_resize_before_ready", async () => {
    let release!: (v: unknown) => void;
    commands.scan_status = () => new Promise((r) => (release = r));
    vi.mocked(HTMLElement.prototype.getBoundingClientRect).mockReturnValue({ height: 2 } as DOMRect);
    render(<App />);
    expect(await screen.findByText("Memuat…")).toBeInTheDocument();
    resizeCb();
    expect(invokeMock).not.toHaveBeenCalledWith("resize_popover", expect.anything());
    vi.mocked(HTMLElement.prototype.getBoundingClientRect).mockReturnValue({ height: 412 } as DOMRect);
    await act(async () => release(status()));
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("resize_popover", { height: 412 }));
    const heights = invokeMock.mock.calls.filter((c) => c[0] === "resize_popover").map((c) => (c[1] as { height: number }).height);
    expect(heights.every((h) => h >= 200)).toBe(true);
  });

  it("startup_failure_shows_retry", async () => {
    const user = userEvent.setup();
    commands.scan_status = () => {
      throw "db terkunci";
    };
    render(<App />);
    expect(await screen.findByText(/db terkunci/)).toBeInTheDocument();
    commands.scan_status = () => status();
    await user.click(screen.getByRole("button", { name: "Coba Lagi" }));
    expect(await screen.findAllByRole("tab")).toHaveLength(5);
  });
});

describe("switching tabs", () => {
  const count = (cmd: string) => invokeMock.mock.calls.filter(([c]) => c === cmd).length;
  const panel = (name: string) => document.getElementById(screen.getByRole("tab", { name }).getAttribute("aria-controls")!)!;

  beforeEach(() => {
    commands.get_overview = () => overviewFixture;
    commands.get_sessions = () => sessionFixture;
    commands.get_models = () => new Promise(() => {});
    commands.get_projects = () => new Promise(() => {});
    commands.get_history = () => new Promise(() => {});
  });

  it("coming back to a tab paints its data right away, without Memuat… or a refetch", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Hari Ini per Jam");
    const fetched = count("get_overview");

    await user.click(screen.getByRole("tab", { name: "Mutasi" }));
    expect(await screen.findByText("Hari Ini · Jum, 30 Okt")).toBeVisible();
    await user.click(screen.getByRole("tab", { name: "Ringkasan" }));
    expect(screen.getByText("Hari Ini per Jam")).toBeVisible();
    expect(within(panel("Ringkasan")).queryByText("Memuat…")).not.toBeInTheDocument();
    expect(count("get_overview")).toBe(fetched);
  });

  it("the first visit to a tab also has its data ready", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Hari Ini per Jam");
    await waitFor(() => expect(count("get_sessions")).toBe(1));
    await act(async () => {});
    await user.click(screen.getByRole("tab", { name: "Mutasi" }));
    expect(screen.getByText("Hari Ini · Jum, 30 Okt")).toBeVisible();
  });

  it("only the active panel takes up room, so only it is measured", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Hari Ini per Jam");
    expect(panel("Ringkasan")).not.toHaveAttribute("hidden");
    for (const name of ["Mutasi", "Model", "Project", "Riwayat"]) expect(panel(name)).toHaveAttribute("hidden");
    await user.click(screen.getByRole("tab", { name: "Mutasi" }));
    expect(panel("Mutasi")).not.toHaveAttribute("hidden");
    expect(panel("Ringkasan")).toHaveAttribute("hidden");
  });

  it("hidden tabs skip data-updated and catch up once when shown, keeping their data on screen", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText("Hari Ini per Jam");
    await waitFor(() => expect(count("get_sessions")).toBe(1));
    const overviews = count("get_overview");

    act(() => emit("data-updated"));
    act(() => emit("data-updated"));
    await waitFor(() => expect(count("get_overview")).toBeGreaterThan(overviews));
    expect(count("get_sessions")).toBe(1);

    let release!: (v: unknown) => void;
    commands.get_sessions = () => new Promise((r) => (release = r));
    await user.click(screen.getByRole("tab", { name: "Mutasi" }));
    await waitFor(() => expect(count("get_sessions")).toBe(2));
    expect(screen.getByText("Hari Ini · Jum, 30 Okt")).toBeVisible();
    expect(within(panel("Mutasi")).queryByText("Memuat…")).not.toBeInTheDocument();
    await act(async () => release(sessionFixture));

    // Nothing new since: another round trip refetches nothing.
    await user.click(screen.getByRole("tab", { name: "Ringkasan" }));
    await user.click(screen.getByRole("tab", { name: "Mutasi" }));
    expect(count("get_sessions")).toBe(2);
  });

  it("a tab keeps its filters across a switch", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("tab", { name: "Mutasi" }));
    await screen.findByText("Hari Ini · Jum, 30 Okt");
    await user.click(screen.getByRole("combobox", { name: "Filter agent" }));
    await user.click(await screen.findByRole("option", { name: "Codex" }));
    await user.click(screen.getByRole("tab", { name: "Ringkasan" }));
    await user.click(screen.getByRole("tab", { name: "Mutasi" }));
    expect(screen.getByRole("combobox", { name: "Filter agent" })).toHaveTextContent("Codex");
  });
});

describe("theme", () => {
  afterEach(() => {
    setTheme("system");
    resetUsdToIdrCache();
  });

  it("applies the chosen theme and follows a change from Pengaturan without a restart", async () => {
    resetUsdToIdrCache();
    commands.get_settings = () => ({ ...settingsFixture, onboarded: true, theme: "dark" });
    render(<App />);
    await screen.findAllByRole("tab");
    await waitFor(() => expect(document.documentElement.classList.contains("dark")).toBe(true));
    commands.get_settings = () => ({ ...settingsFixture, onboarded: true, theme: "light" });
    await act(async () => {
      emit("data-updated");
    });
    await waitFor(() => expect(document.documentElement.classList.contains("dark")).toBe(false));
  });
});

describe("onboarding", () => {
  it("shows Welcome instead of the tabs while onboarded is false, then the tabs after Mulai", async () => {
    commands.get_settings = () => ({ ...settingsFixture, onboarded: false });
    commands.get_overview = () => {
      throw new Error("unused");
    };
    render(<App />);
    expect(await screen.findByRole("button", { name: "Mulai" })).toBeInTheDocument();
    expect(screen.queryAllByRole("tab")).toHaveLength(0);
    await userEvent.click(screen.getByRole("button", { name: "Mulai" }));
    expect(await screen.findAllByRole("tab")).toHaveLength(5);
    expect(invokeMock).toHaveBeenCalledWith("save_settings", { settings: { onboarded: true } });
  });
});
