/// <reference types="node" />
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// vitest runs from the project root
const css = readFileSync("src/index.css", "utf8");
const tokens = readFileSync("src/styles/tokens.css", "utf8");

const block = (source: string, selector: string) => {
  const start = source.indexOf(`${selector} {`);
  return source.slice(start, source.indexOf("}", start));
};
const hex = (b: string, name: string) => b.match(new RegExp(`--${name}:\\s*(#[0-9a-fA-F]{6})`))?.[1] ?? "";

function luminance(h: string) {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16) / 255).map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
const ratio = (a: string, b: string) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

describe("focus ring and status text contrast", () => {
  for (const [mode, selector] of [["light", ":root"], ["dark", ".dark"]] as const) {
    it(`the ${mode} focus ring is at least 3:1 on the surfaces it sits on`, () => {
      const b = block(css, selector);
      const ring = hex(b, "ring");
      expect(ring, "--ring must be a hex colour so it can be checked").not.toBe("");
      for (const surface of ["background", "card", "muted"]) expect(ratio(ring, hex(b, surface)), surface).toBeGreaterThanOrEqual(3);
    });
  }

  it("the connected status text is at least 4.5:1 in both modes", () => {
    for (const [selector, surface] of [[":root", "#ffffff"], [".dark", "#0f0f11"]] as const) {
      const ok = hex(block(tokens, selector), "ok");
      expect(ok, `${selector} --ok`).not.toBe("");
      expect(ratio(ok, surface)).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("the focus styles use the ring at full strength", () => {
    expect(css).not.toContain("outline-ring/50");
  });
});
