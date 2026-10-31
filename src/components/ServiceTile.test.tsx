import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import antigravitySvg from "@/assets/logos/antigravity.svg?raw";
import openrouterSvg from "@/assets/logos/openrouter.svg?raw";
import { Logo, LogoSprite } from "./Logo";
import { ServiceTile } from "./ServiceTile";

describe("ServiceTile", () => {
  it("Antigravity shows its logo in Google blue through the same tint pattern, with no agent color", () => {
    const { container } = render(
      <>
        <LogoSprite />
        <ServiceTile service="antigravity" size={26} />
      </>,
    );
    const tile = container.querySelector<HTMLElement>("span")!;
    expect(tile.querySelector("svg use")).toHaveAttribute("href", "#lg-antigravity");
    expect(container.querySelector("symbol#lg-antigravity path")).not.toBeNull();
    expect(screen.queryByText("AG")).not.toBeInTheDocument();
    expect(tile.style.width).toBe("26px");
    expect(tile.style.background).toContain("color-mix(in oklab, var(--svc-ag) 15%, var(--background))");
    expect(tile.style.color).toBe("var(--svc-ag)");
    expect(tile.outerHTML).not.toContain("--a-");
  });

  it("OpenRouter stays neutral", () => {
    const { container } = render(<ServiceTile service="openrouter" />);
    expect((container.firstElementChild as HTMLElement).className).toContain("text-muted-foreground");
  });

  it("OpenRouter shows its logo from the sprite", () => {
    const { container } = render(
      <>
        <LogoSprite />
        <ServiceTile service="openrouter" />
      </>,
    );
    expect(container.querySelector("svg use")).toHaveAttribute("href", "#lg-openrouter");
    expect(container.querySelector("symbol#lg-openrouter path")).not.toBeNull();
    expect(container.outerHTML).not.toContain("--a-");
  });

  it("the logo works as a plain Logo too", () => {
    const { container } = render(<Logo id="openrouter" size={20} />);
    expect(container.querySelector("svg use")).toHaveAttribute("href", "#lg-openrouter");
  });
});

describe("OpenRouter logo files", () => {
  it("is monochrome like the others, 1em, and holds only SVG shapes", () => {
    for (const svg of [openrouterSvg, antigravitySvg]) {
      expect(svg).toContain('fill="currentColor"');
      expect(svg).toContain('height="1em"');
      expect(svg).toContain('width="1em"');
      expect(svg).not.toMatch(/<script|<foreignObject|<image|<a[ >]|\son\w+=|href=|javascript:/i);
      const tags = [...svg.matchAll(/<\/?([a-zA-Z]+)/g)].map((m) => m[1]);
      expect([...new Set(tags)].sort()).toEqual(["path", "svg", "title"]);
      expect(svg).not.toMatch(/#[0-9a-f]{3,8}\b/i); // no hard-coded color left
    }
  });
});
