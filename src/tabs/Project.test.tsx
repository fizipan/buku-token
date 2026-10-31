import { render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import projectsFixture from "../test/fixtures/projects-view.json";
import settingsFixture from "../test/fixtures/settings.json";
import { commands, invokeMock, listenMock, resetTauri } from "../test/tauri";
import type { ProjectRow, ProjectsView } from "../lib/api";
import { resetUsdToIdrCache } from "../hooks/useUsdToIdr";

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: [string, unknown?]) => invokeMock(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: Parameters<typeof listenMock>) => listenMock(...a) }));

import { Project } from "./Project";

const prow = (name: string, over: Partial<ProjectRow> = {}): ProjectRow => ({
  project: { path: `/p/${name}`, name, displayPath: `~/p/${name}` },
  byAgent: [5, 0, 0, 0, 0],
  costUsd: 5,
  unpricedTokens: 0,
  sessions: 14,
  share: 39,
  folded: 0,
  ...over,
});
const view = (over: Partial<ProjectsView> = {}): ProjectsView => ({ ...(projectsFixture as unknown as ProjectsView), ...over });

beforeEach(() => {
  resetTauri();
  resetUsdToIdrCache();
  commands.get_settings = () => ({ ...settingsFixture, usdToIdr: 16300 });
  commands.get_projects = () => view();
});

describe("Project", () => {
  it("keeps the list inside the popover when a path is very long", async () => {
    const long = "Acme/inventory-app/.claude/worktrees/objective-banach-44214c-very-long-path";
    commands.get_projects = () => view({ rows: [prow("inventory-app", { project: { path: "/p/t", name: "inventory-app", displayPath: long } })] });
    render(<Project />);
    const path = await screen.findByText(long);
    // An implicit `auto` grid track grows to the nowrap text's width; minmax(0,1fr) lets it truncate instead.
    expect(path.closest("ul")).toHaveClass("grid-cols-1");
    expect(path).toHaveClass("truncate");
  });

  it("shows the header with total and project count (folded rows counted)", async () => {
    render(<Project />);
    expect(await screen.findByText("Biaya per Project")).toBeInTheDocument();
    // 1 real project + 3 folded
    expect(screen.getByText("$20,00 · Rp326.000 · 4 project")).toBeInTheDocument();
  });

  it("header wraps inside its column instead of overflowing", async () => {
    render(<Project />);
    const text = await screen.findByText("$20,00 · Rp326.000 · 4 project");
    expect(text.parentElement?.parentElement).toHaveClass("min-w-0");
  });

  it("project with only unpriced tokens: a dash and 'Harga Belum Ada', never $0,00", async () => {
    commands.get_projects = () =>
      view({ rows: [prow("buku-token", { costUsd: 0, byAgent: [0, 0, 0, 0, 0], unpricedTokens: 900 })], totalCostUsd: 0, insight: null });
    render(<Project />);
    await screen.findByText("Biaya per Project");
    expect(screen.getAllByText("Harga Belum Ada")).toHaveLength(2);
    expect(screen.queryByText(/\$0,00/)).not.toBeInTheDocument();
  });

  it("renders a row with name, cost, path and 'N sesi · share'", async () => {
    commands.get_projects = () => view({ rows: [prow("buku-token", { costUsd: 7.5 })] });
    render(<Project />);
    const li = (await screen.findAllByRole("listitem"))[0];
    expect(within(li).getByText("buku-token")).toBeInTheDocument();
    expect(within(li).getByText("$7,50")).toBeInTheDocument();
    expect(within(li).getByText("~/p/buku-token")).toBeInTheDocument();
    expect(within(li).getByText("14 sesi · 39%")).toBeInTheDocument();
    expect(within(li).getByRole("img", { name: /Claude Code \$5,00/ })).toBeInTheDocument();
  });

  it("renders 6 rows plus the folded 'N Project Lainnya' row", async () => {
    const rows = ["a", "b", "c", "d", "e", "f"].map((n) => prow(n));
    rows.push(prow("Project Lainnya", { folded: 3, sessions: 2, project: { path: "", name: "Project Lainnya", displayPath: "" } }));
    commands.get_projects = () => view({ rows, insight: null });
    render(<Project />);
    await screen.findByText("Biaya per Project");
    const legend = screen.getByTestId("project-legend");
    const rowItems = screen.getAllByRole("listitem").filter((li) => !legend.contains(li));
    expect(rowItems).toHaveLength(7);
    expect(screen.getByText("3 Project Lainnya")).toBeInTheDocument();
  });

  it("keys rows by folded so 'Tanpa Project' and the folded row (both path '') do not collide", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const empty = { path: "", name: "Tanpa Project", displayPath: "" };
    commands.get_projects = () =>
      view({
        rows: [prow("x", { project: empty }), prow("y", { project: { ...empty, name: "Project Lainnya" }, folded: 2 })],
        insight: null,
      });
    render(<Project />);
    await screen.findByText("Tanpa Project");
    expect(screen.getByText("2 Project Lainnya")).toBeInTheDocument();
    expect(spy.mock.calls.flat().join(" ")).not.toMatch(/same key|unique "key"/);
    spy.mockRestore();
  });

  it("shows a legend of the agents that have usage", async () => {
    render(<Project />);
    await screen.findByText("Biaya per Project");
    const legend = screen.getByTestId("project-legend");
    expect(within(legend).getByText("Claude Code")).toBeInTheDocument();
    expect(within(legend).getByText("Codex")).toBeInTheDocument();
    expect(within(legend).queryByText("Hermes")).not.toBeInTheDocument();
  });

  it("renders the insight callout with 'naik'", async () => {
    render(<Project />);
    const note = await screen.findByRole("note");
    expect(note).toHaveTextContent("buku-token memakai 50% biaya periode ini, naik dari 35% periode sebelumnya.");
    expect(within(note).getByText("50% biaya").tagName).toBe("B");
  });

  it("says 'turun' and 'sama dengan' when appropriate", async () => {
    const base = projectsFixture.insight as unknown as NonNullable<ProjectsView["insight"]>;
    commands.get_projects = () => view({ insight: { ...base, share: 30, prevShare: 35 } });
    const { unmount } = render(<Project />);
    expect(await screen.findByRole("note")).toHaveTextContent("turun dari 35% periode sebelumnya");
    unmount();
    commands.get_projects = () => view({ insight: { ...base, share: 35, prevShare: 35 } });
    render(<Project />);
    expect(await screen.findByRole("note")).toHaveTextContent("buku-token memakai 35% biaya periode ini, sama dengan periode sebelumnya.");
  });

  it("omits the callout without insight", async () => {
    commands.get_projects = () => view({ insight: null });
    render(<Project />);
    await screen.findByText("Biaya per Project");
    expect(screen.queryByRole("note")).not.toBeInTheDocument();
  });
});
