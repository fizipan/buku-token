import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "@/lib/i18n";
import { StatusBar } from "./StatusBar";

const show = (props: Partial<React.ComponentProps<typeof StatusBar>> = {}) => {
  const onRefresh = vi.fn();
  render(
    <I18nProvider lang="id">
      <StatusBar lastScanAt={null} refreshing={false} onRefresh={onRefresh} onOpenSettings={() => {}} {...props} />
    </I18nProvider>,
  );
  return { onRefresh, button: screen.getByRole("button", { name: "Perbarui" }) };
};

describe("StatusBar", () => {
  it("a background scan turns the icon but does not take Refresh away", async () => {
    const { button, onRefresh } = show({ scanning: true });
    expect(button).not.toHaveAttribute("aria-disabled");
    expect(button).toBeEnabled();
    expect(button.querySelector("svg")).toHaveClass("motion-safe:animate-spin");
    await userEvent.click(button);
    expect(onRefresh).toHaveBeenCalledTimes(1);
  });

  it("our own refresh keeps the button focusable, and ignores a second press", async () => {
    const { button, onRefresh } = show({ refreshing: true });
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button).toBeEnabled();
    button.focus();
    await userEvent.keyboard("{Enter}");
    expect(onRefresh).not.toHaveBeenCalled();
    expect(button).toHaveFocus();
  });
});
