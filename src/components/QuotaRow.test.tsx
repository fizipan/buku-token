import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { QuotaWindow } from "@/lib/api";
import { I18nProvider } from "@/lib/i18n";
import { QuotaRow } from "./QuotaRow";

const now = new Date("2026-11-01T00:00:00Z");
const win = (over: Partial<QuotaWindow> = {}): QuotaWindow => ({
  id: "total",
  label: "Total",
  usedPct: 40,
  resetsAt: "2026-11-01T05:00:00Z",
  observedAt: "2026-11-01T00:00:00Z",
  expired: false,
  ...over,
});
const show = (w: QuotaWindow, pace: number | null = null, extra: Partial<React.ComponentProps<typeof QuotaRow>> = {}) =>
  render(
    <TooltipProvider>
      <QuotaRow agent="cr" window={w} pace={pace} now={now} {...extra} />
    </TooltipProvider>,
  );

describe("QuotaRow", () => {
  it("widens the value column in Terpakai mode, in both grid variants", () => {
    const grid = () => screen.getByTestId("quota-row").className;
    const cases: [boolean, "left" | "used", string][] = [
      [false, "left", "grid-cols-[92px_1fr_58px_62px]"],
      [true, "left", "grid-cols-[92px_1fr_58px]"],
      [false, "used", "grid-cols-[92px_1fr_88px_62px]"],
      [true, "used", "grid-cols-[92px_1fr_88px]"],
    ];
    for (const [hideReset, quotaDisplay, want] of cases) {
      const { unmount } = show(win(), null, { hideReset, quotaDisplay });
      expect(grid()).toContain(want);
      unmount();
    }
  });

  it("the focus stop is the one progressbar, with its value in words, and the bar inside is hidden", () => {
    show(win(), 50);
    const bar = screen.getByRole("progressbar", { name: "Total" });
    expect(bar).toHaveAttribute("tabindex", "0");
    expect(bar).toHaveAttribute("aria-valuenow", "40");
    expect(bar).toHaveAttribute("aria-valuetext", "Total: terpakai 40%, sisa 60%");
    const inner = bar.querySelector("[data-slot=progress]")!;
    expect(inner).toHaveAttribute("aria-hidden", "true");
    expect(screen.getAllByRole("progressbar")).toHaveLength(1);
  });

  it("shows label, remaining share and no pace tick without a pace", () => {
    show(win());
    expect(screen.getByText("Total")).toBeInTheDocument();
    expect(screen.getByText("Sisa 60%")).toBeInTheDocument();
    expect(screen.queryByTestId("pace-tick")).not.toBeInTheDocument();
  });

  it("turns the share red at 15% or less remaining", () => {
    show(win({ usedPct: 85 }));
    expect(screen.getByText("Sisa 15%").className).toMatch(/destructive/);
  });

  it("a stale window shows the data date in red with an accessible name", () => {
    show(win({ observedAt: "2026-10-28T08:00:00Z" }));
    const date = screen.getByTitle("Data Terakhir 28 Okt");
    expect(date).toHaveTextContent("Data Terakhir 28 Okt");
    expect(date.querySelector(".sr-only")).toHaveTextContent("Data Terakhir");
    expect(date.className).toMatch(/destructive/);
    expect(date).toHaveAttribute("title", "Data Terakhir 28 Okt");
  });

  it("draws the pace tick when given a pace", () => {
    show(win(), 50);
    expect(screen.getByTestId("pace-tick")).toHaveStyle({ left: "calc(50% - 1px)" });
  });

  it("Terpakai shows the used share, and the red mark still follows what is left", () => {
    const { unmount } = show(win(), null, { quotaDisplay: "used" });
    expect(screen.getByText("Terpakai 40%").className).not.toMatch(/destructive/);
    expect(screen.queryByText(/Sisa/)).not.toBeInTheDocument();
    unmount();
    show(win({ usedPct: 90 }), null, { quotaDisplay: "used" });
    expect(screen.getByText("Terpakai 90%").className).toMatch(/destructive/);
  });

  it("Hitung Mundur shows the time left, in days from one day out", () => {
    const { unmount } = show(win({ resetsAt: "2026-11-01T02:15:00Z" }), null, { resetDisplay: "countdown" });
    expect(screen.getByText("2j 15m")).toBeInTheDocument();
    unmount();
    show(win({ resetsAt: "2026-11-04T05:00:00Z" }), null, { resetDisplay: "countdown" });
    expect(screen.getByText("3 hari")).toBeInTheDocument();
  });

  it("Jam keeps the clock time for a reset today", () => {
    show(win({ resetsAt: "2026-11-01T02:15:00Z" }), null, { resetDisplay: "clock" });
    expect(screen.queryByText("2j 15m")).not.toBeInTheDocument();
    expect(screen.getByText(/^\d\d[.:]\d\d$/)).toBeInTheDocument();
  });

  it("English needs less room: the Used column is 64px, not 88px, and Left keeps 58px", () => {
    const grid = () => screen.getByTestId("quota-row").className;
    const render_ = (props: Partial<React.ComponentProps<typeof QuotaRow>>) =>
      render(
        <I18nProvider lang="en">
          <TooltipProvider>
            <QuotaRow agent="cr" window={win()} now={now} {...props} />
          </TooltipProvider>
        </I18nProvider>,
      );
    const cases: [boolean, "left" | "used", string, string][] = [
      [false, "left", "grid-cols-[92px_1fr_58px_62px]", "Left 60%"],
      [true, "left", "grid-cols-[92px_1fr_58px]", "Left 60%"],
      [false, "used", "grid-cols-[92px_1fr_64px_62px]", "Used 40%"],
      [true, "used", "grid-cols-[92px_1fr_64px]", "Used 40%"],
    ];
    for (const [hideReset, quotaDisplay, want, text] of cases) {
      const { unmount } = render_({ hideReset, quotaDisplay });
      expect(grid()).toContain(want);
      expect(screen.getByTestId("quota-row")).toHaveTextContent(text);
      unmount();
    }
  });

  it("words the pace, the countdown and the expired sentence in English", () => {
    const { unmount } = render(
      <I18nProvider lang="en">
        <TooltipProvider>
          <QuotaRow agent="cr" window={win({ resetsAt: "2026-11-04T00:00:00Z" })} pace={10} now={now} resetDisplay="countdown" />
        </TooltipProvider>
      </I18nProvider>,
    );
    expect(screen.getByTestId("quota-row")).toHaveTextContent("3d");
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuetext", "Total: used 40%, left 60%");
    unmount();
    render(
      <I18nProvider lang="en">
        <TooltipProvider>
          <QuotaRow agent="cr" window={win({ expired: true })} now={now} />
        </TooltipProvider>
      </I18nProvider>,
    );
    expect(screen.getByText("Already reset · no new data yet")).toBeInTheDocument();
  });
});
