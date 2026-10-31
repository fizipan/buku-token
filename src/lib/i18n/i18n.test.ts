import { describe, expect, it } from "vitest";
import { DICTIONARIES, translator, type Key } from ".";
import { en } from "./en";
import { id } from "./id";

describe("dictionaries", () => {
  it("have identical keys", () => {
    expect(Object.keys(en).sort()).toEqual(Object.keys(id).sort());
  });

  it("have no empty value", () => {
    for (const [lang, dict] of Object.entries(DICTIONARIES)) {
      for (const [key, value] of Object.entries(dict)) {
        expect(value.trim(), `${lang}.${key}`).not.toBe("");
      }
    }
  });

  it("use the same placeholders and bold markers in both languages", () => {
    const shape = (s: string) => ({ vars: [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort(), bold: (s.match(/<b>/g) ?? []).length });
    for (const key of Object.keys(id) as Key[]) {
      expect(shape(en[key]), key).toEqual(shape(id[key]));
    }
  });

  it("translate a few known texts", () => {
    expect(translator("id")("quotaLeft", { n: 62 })).toBe("Sisa 62%");
    expect(translator("en")("quotaLeft", { n: 62 })).toBe("Left 62%");
    expect(translator("en")("quotaUsed", { n: 74 })).toBe("Used 74%");
  });
});

describe("rich text", () => {
  it("bolds the template's markers and leaves values as text", () => {
    const nodes = translator("en").rich("projectInsight", { name: "<b>evil</b>", share: "40%", trend: "up" }) as { props: { children: string }; type: unknown }[];
    const bold = nodes.filter((n) => n.type === "b").map((n) => n.props.children);
    expect(bold).toEqual(["<b>evil</b>", "40% of cost"]);
  });
});
