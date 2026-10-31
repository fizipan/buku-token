import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import modelsFixture from "../test/fixtures/models-view.json";
import settingsFixture from "../test/fixtures/settings.json";
import { commands, invokeMock, listenMock, resetTauri } from "../test/tauri";
import type { ModelRow, ModelsView } from "../lib/api";
import { resetUsdToIdrCache } from "../hooks/useUsdToIdr";

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: [string, unknown?]) => invokeMock(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: Parameters<typeof listenMock>) => listenMock(...a) }));

import { Model } from "./Model";

const view = (over: Partial<ModelsView> = {}): ModelsView => ({ ...(modelsFixture as unknown as ModelsView), ...over });
const mrow = (over: Partial<ModelRow>): ModelRow => ({ ...(modelsFixture.rows[0] as unknown as ModelRow), ...over });

beforeEach(() => {
  resetTauri();
  resetUsdToIdrCache();
  commands.get_settings = () => ({ ...settingsFixture, usdToIdr: 16300 });
  commands.get_models = () => view();
});

describe("Model", () => {
  it("shows the header with total cost in USD and IDR", async () => {
    render(<Model />);
    expect(await screen.findByText("Biaya per Model")).toBeInTheDocument();
    expect(screen.getByText("$12,00 · Rp195.600")).toBeInTheDocument();
  });

  it("total with only unpriced tokens: a dash and 'Harga Belum Ada', never $0,00", async () => {
    commands.get_models = () =>
      view({ rows: [mrow({ model: "big-pickle", agent: "oc", costUsd: null, share: null })], totalCostUsd: 0, unpricedTokens: 1000, cacheInsight: null });
    render(<Model />);
    const header = (await screen.findByText("Biaya per Model")).parentElement!;
    expect(within(header).getByText("–")).toBeInTheDocument();
    expect(within(header).getByText("Harga Belum Ada")).toBeInTheDocument();
    expect(screen.queryByText(/\$0,00/)).not.toBeInTheDocument();
  });

  it("orders rows by cost, unknown cost last", async () => {
    commands.get_models = () =>
      view({
        rows: [
          mrow({ model: "big-pickle", agent: "oc", costUsd: null, share: null }),
          mrow({ model: "claude-haiku-4-5", costUsd: 1, share: 8 }),
          mrow({ model: "claude-opus-5-5", costUsd: 12, share: 80 }),
        ],
      });
    render(<Model />);
    await screen.findByText("Biaya per Model");
    const names = screen.getAllByRole("listitem").map((li) => li.querySelector("[data-model]")?.textContent);
    expect(names).toEqual(["Opus 5.5", "Haiku 4.5", "big-pickle"]);
  });

  it("shows row details: token sub-line and share", async () => {
    render(<Model />);
    const li = (await screen.findAllByRole("listitem"))[0];
    expect(within(li).getByText("$12,00")).toBeInTheDocument();
    expect(within(li).getByText("Input 100 · Output 200 · Cache 700")).toBeInTheDocument();
    expect(within(li).getByText("80%")).toBeInTheDocument();
    expect(within(li).getByRole("img", { name: /Claude Code/ })).toBeInTheDocument();
  });

  it("shows an unknown cost as a dash with 'Harga Belum Ada', never $0,00", async () => {
    render(<Model />);
    const li = (await screen.findAllByRole("listitem"))[1];
    expect(within(li).getByText("–")).toBeInTheDocument();
    expect(within(li).getByText("Harga Belum Ada")).toBeInTheDocument();
    expect(screen.queryByText("$0,00")).not.toBeInTheDocument();
  });

  it("shows the token/cost mix bars with legend", async () => {
    render(<Model />);
    expect(await screen.findByText("Ke Mana Token Pergi")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /Token: cache read 60%, input 10%, output 20%, cache write 10%/ })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /Biaya: cache read 20%/ })).toBeInTheDocument();
    expect(screen.getByText("Cache read")).toBeInTheDocument();
    expect(screen.getByText("Input · Output · Cache write")).toBeInTheDocument();
  });

  it("renders the cache callout with bold parts", async () => {
    render(<Model />);
    const note = await screen.findByRole("note");
    expect(note).toHaveTextContent("Cache read memakai 60% token, tapi cuma 20% biaya. Tanpa cache, periode ini kira-kira $40,00.");
    expect(within(note).getByText("60% token").tagName).toBe("B");
    expect(within(note).getByText("20% biaya").tagName).toBe("B");
    expect(within(note).getByText("$40,00").tagName).toBe("B");
  });

  it("omits the callout without cacheInsight", async () => {
    commands.get_models = () => view({ cacheInsight: null });
    render(<Model />);
    await screen.findByText("Biaya per Model");
    expect(screen.queryByRole("note")).not.toBeInTheDocument();
  });

  it("refetches with the chosen period", async () => {
    const user = userEvent.setup();
    render(<Model />);
    await screen.findByText("Biaya per Model");
    await user.click(screen.getByRole("combobox", { name: "Periode" }));
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["Hari Ini", "7 Hari Terakhir", "30 Hari Terakhir"]);
    await user.click(screen.getByRole("option", { name: "30 Hari Terakhir" }));
    await vi.waitFor(() =>
      expect(invokeMock.mock.calls.filter(([c]) => c === "get_models").slice(-1)[0]?.[1]).toEqual({ period: "30d" }),
    );
  });

  it("shows an empty state without rows", async () => {
    commands.get_models = () => view({ rows: [], cacheInsight: null, totalCostUsd: 0 });
    render(<Model />);
    expect(await screen.findByText("Belum ada pemakaian model")).toBeInTheDocument();
  });
});
