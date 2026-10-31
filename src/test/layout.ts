import { vi } from "vitest";

/** jsdom has no layout: report a fixed size so Recharts' ResponsiveContainer renders the chart. Undo with vi.unstubAllGlobals(). */
export function stubChartLayout(width = 380, height = 132) {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(private cb: ResizeObserverCallback) {}
      observe(target: Element) {
        this.cb([{ target, contentRect: { width, height } } as ResizeObserverEntry], this as unknown as ResizeObserver);
      }
      unobserve() {}
      disconnect() {}
    },
  );
}
