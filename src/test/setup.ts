import "@testing-library/jest-dom/vitest";
import { configure } from "@testing-library/react";

// findBy* and waitFor give up after 1 s by default, which a loaded machine can exceed.
configure({ asyncUtilTimeout: 5000 });

// Recharts' ResponsiveContainer needs ResizeObserver, which jsdom lacks.
class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver = ResizeObserverMock;

// Radix Select/Popper in jsdom: pointer capture and scrollIntoView are missing.
Element.prototype.hasPointerCapture ??= () => false;
Element.prototype.setPointerCapture ??= () => {};
Element.prototype.releasePointerCapture ??= () => {};
Element.prototype.scrollIntoView ??= () => {};
