import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AgentTile } from "./AgentTile";
import { Logo, LogoSprite } from "./Logo";

describe("AgentTile", () => {
  it("oc renders the OC monogram", () => {
    render(<AgentTile agent="oc" />);
    expect(screen.getByText("OC")).toBeInTheDocument();
  });

  it("cc renders an svg using #lg-claude, tinted with the agent color", () => {
    const { container } = render(<AgentTile agent="cc" />);
    const use = container.querySelector("svg use");
    expect(use).toHaveAttribute("href", "#lg-claude");
    const tile = container.firstElementChild as HTMLElement;
    expect(tile.style.width).toBe("24px");
    expect(tile.style.background).toContain("color-mix(in oklab, var(--a-cc) 15%, var(--background))");
    expect(tile.style.color).toBe("var(--a-cc)");
  });

  it("model overrides the logo (codex tile showing a claude model)", () => {
    const { container } = render(<AgentTile agent="oc" model="kimi-k2" />);
    expect(container.querySelector("svg use")).toHaveAttribute("href", "#lg-kimi");
  });

  it("size prop sets the tile size", () => {
    const { container } = render(<AgentTile agent="cx" size={32} />);
    expect((container.firstElementChild as HTMLElement).style.width).toBe("32px");
  });
});

describe("LogoSprite", () => {
  it("defines one symbol per logo, hidden", () => {
    const { container } = render(<LogoSprite />);
    for (const id of ["claude", "openai", "kimi", "minimax", "nousresearch"]) {
      expect(container.querySelector(`symbol#lg-${id}`)).not.toBeNull();
    }
    expect(container.querySelector("symbol#lg-claude")?.getAttribute("viewBox")).toBe("0 0 24 24");
    expect(container.querySelector("title")).toBeNull();
    expect(container.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
  });
  it("Logo references the symbol", () => {
    const { container } = render(<Logo id="minimax" size={14} />);
    expect(container.querySelector("use")).toHaveAttribute("href", "#lg-minimax");
  });
});
