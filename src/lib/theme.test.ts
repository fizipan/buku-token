import { afterEach, describe, expect, it, vi } from "vitest";
import { setTheme, syncColorScheme } from "./theme";

type Listener = (e: { matches: boolean }) => void;

function mockMedia(initial: boolean) {
  let listener: Listener | undefined;
  const mq = {
    matches: initial,
    addEventListener: (_: string, l: Listener) => (listener = l),
    removeEventListener: vi.fn(),
  };
  vi.stubGlobal("matchMedia", () => mq);
  return { mq, fire: (matches: boolean) => listener?.({ matches }) };
}

afterEach(() => {
  setTheme("system");
  vi.unstubAllGlobals();
  document.documentElement.classList.remove("dark");
});

describe("syncColorScheme", () => {
  it("applies .dark when macOS is dark and follows changes", () => {
    const m = mockMedia(true);
    const stop = syncColorScheme();
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    m.fire(false);
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    m.fire(true);
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    stop();
    expect(m.mq.removeEventListener).toHaveBeenCalled();
  });
  it("stays light when macOS is light", () => {
    mockMedia(false);
    syncColorScheme();
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });
});

describe("setTheme", () => {
  const dark = () => document.documentElement.classList.contains("dark");

  it("Gelap and Terang override macOS, Sistem follows it again", () => {
    const m = mockMedia(false);
    syncColorScheme();
    setTheme("dark");
    expect(dark()).toBe(true);
    m.fire(false);
    expect(dark()).toBe(true);
    setTheme("light");
    m.fire(true);
    expect(dark()).toBe(false);
    setTheme("system");
    expect(dark()).toBe(true);
    m.fire(false);
    expect(dark()).toBe(false);
  });
});
