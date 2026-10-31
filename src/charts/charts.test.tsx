import { act, render, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import type { Burn } from "../lib/api";
import { stubChartLayout } from "../test/layout";
import { clock, usd, usd0 } from "../lib/format";
import { AgentDonut } from "./AgentDonut";
import { BurnChart } from "./BurnChart";
import { StackedColumns, yAxisWidth } from "./StackedColumns";

beforeAll(() => stubChartLayout());
afterAll(() => vi.unstubAllGlobals());

const burn = (over: Partial<Burn> = {}): Burn => ({
  windowStart: "2026-10-30T10:00:00Z",
  resetsAt: "2026-10-30T15:00:00Z",
  points: [
    [0, 0],
    [60, 24.5],
    [120, 40],
  ],
  projectedAtReset: 88,
  exhaustsAt: null,
  ...over,
});

describe("chart keyboard focus", () => {
  it("the donut has no focus stop it cannot use, the keyboard charts keep one with a visible ring", () => {
    const donut = render(<AgentDonut byAgent={[3, 2, 0, 0, 1]} centre="$6" aria-label="Biaya" />);
    expect(donut.container.querySelector(".recharts-surface")).not.toHaveAttribute("tabindex");
    const line = render(<BurnChart burn={burn()} />);
    expect(line.container.querySelector(".recharts-surface")).toHaveAttribute("tabindex", "0");
    const cls = line.container.querySelector("[data-slot=chart]")!.className;
    expect(cls).toContain("[&_.recharts-surface:focus-visible]:outline-2");
    expect(cls).not.toContain("[&_.recharts-surface]:outline-hidden");
  });
});

describe("BurnChart", () => {
  it("draws axis labels, a 2px actual line and a dotted projection", () => {
    const { container } = render(<BurnChart burn={burn()} />);
    const chart = within(container);
    expect(chart.getByText("Limit")).toBeInTheDocument();
    expect(chart.getByText("50%")).toBeInTheDocument();
    expect(chart.getByText("Sekarang")).toBeInTheDocument();
    expect(chart.getByText(clock("2026-10-30T10:00:00Z"))).toBeInTheDocument();
    expect(chart.getByText(clock("2026-10-30T15:00:00Z"))).toBeInTheDocument();
    expect(chart.getByText("±88% saat reset")).toBeInTheDocument();
    const dashed = container.querySelector('.recharts-area-curve[stroke-dasharray="2 5"]');
    expect(dashed).not.toBeNull();
    expect(dashed).toHaveAttribute("stroke", "var(--a-cc)");
    expect(container.querySelector('path[stroke-width="2"]:not([stroke-dasharray])')).not.toBeNull();
  });

  it("anchors the first x label to the start and the last to the end so none is clipped", () => {
    const { container } = render(<BurnChart burn={burn()} />);
    const label = (t: string) => within(container).getByText(t).closest("text");
    expect(label(clock("2026-10-30T10:00:00Z"))).toHaveAttribute("text-anchor", "start");
    expect(label(clock("2026-10-30T15:00:00Z"))).toHaveAttribute("text-anchor", "end");
    expect(label("Sekarang")).toHaveAttribute("text-anchor", "middle");
  });

  it("drops Sekarang when it would collide with the start or end label", () => {
    const early = render(<BurnChart burn={burn({ points: [[0, 0], [10, 2]] })} />);
    expect(within(early.container).queryByText("Sekarang")).not.toBeInTheDocument();
    const late = render(<BurnChart burn={burn({ points: [[0, 0], [295, 90]] })} />);
    expect(within(late.container).queryByText("Sekarang")).not.toBeInTheDocument();
  });

  it("keeps the exhaust label inside the plot (below the top edge)", () => {
    const { container } = render(<BurnChart burn={burn({ exhaustsAt: "2026-10-30T13:00:00Z", projectedAtReset: 140 })} />);
    const t = within(container).getByText(/Habis ±/).closest("text")!;
    expect(Number(t.getAttribute("y"))).toBeGreaterThanOrEqual(12);
  });

  it("projection turns red and ends at the exhaust time when the quota runs out", () => {
    const exhaustsAt = "2026-10-30T13:00:00Z";
    const { container } = render(<BurnChart burn={burn({ exhaustsAt, projectedAtReset: 140 })} />);
    expect(container.querySelector('.recharts-area-curve[stroke-dasharray="2 5"]')).toHaveAttribute("stroke", "var(--destructive)");
    expect(within(container).getByText(`Habis ±${clock(exhaustsAt)}`)).toBeInTheDocument();
  });
});

const hours = Array.from({ length: 18 }, (_, i) => ({
  label: String(i + 6).padStart(2, "0"),
  byAgent: i === 6 ? [3.5, 1, 0, 0, 0] : [0, 0, 0, 0, 0],
}));

describe("StackedColumns", () => {
  it("labels every third hour, shows $ ticks and a legend of agents that have data", () => {
    const { container } = render(<StackedColumns data={hours} valueFormat={usd0} highlightIndex={6} />);
    const chart = within(container);
    expect(chart.getByText("06")).toBeInTheDocument();
    expect(chart.getByText("12")).toBeInTheDocument();
    expect(chart.queryByText("07")).not.toBeInTheDocument();
    expect(chart.getByText("$0")).toBeInTheDocument();
    expect(chart.getByText("$4")).toBeInTheDocument();
    expect(chart.getByText("$6")).toBeInTheDocument();
    const legend = chart.getByTestId("stacked-legend");
    expect(legend).toHaveTextContent("Claude Code");
    expect(legend).toHaveTextContent("Codex");
    expect(legend).not.toHaveTextContent("OpenCode");
  });

  it("showLegend={false} hides the legend", () => {
    const { container } = render(<StackedColumns data={hours} valueFormat={usd0} showLegend={false} />);
    expect(within(container).queryByTestId("stacked-legend")).not.toBeInTheDocument();
  });

  it("sizes the y axis to its longest label so large amounts are not clipped", () => {
    // 6px margin + 7.5px per character + 2px breathing room
    expect(yAxisWidth(["$0", "$6"])).toBe(23);
    expect(yAxisWidth(["$0", "$200", "$400"])).toBe(38);
    expect(yAxisWidth(["$0", "$1.000", "$2.000"])).toBe(53);
  });

  it("honours tickStep", () => {
    const { container } = render(<StackedColumns data={hours} valueFormat={usd0} tickStep={5} />);
    expect(within(container).getByText("$5")).toBeInTheDocument();
    expect(within(container).queryByText("$2")).not.toBeInTheDocument();
  });

  it("the chart is keyboard focusable and shows the tooltip with a total", async () => {
    const { container } = render(<StackedColumns data={hours} valueFormat={usd0} tooltipFormat={usd} />);
    const surface = container.querySelector<SVGElement>(".recharts-surface")!;
    expect(surface).toHaveAttribute("tabindex", "0");
    act(() => surface.focus());
    const user = userEvent.setup();
    for (let i = 0; i < 6; i++) await user.keyboard("{ArrowRight}");
    expect(await within(container).findByText("Jumlah")).toBeInTheDocument();
    expect(within(container).getByText("$4,50")).toBeInTheDocument();
  });
});
